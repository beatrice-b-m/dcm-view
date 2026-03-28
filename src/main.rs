use anyhow::{Context, Result};
use clap::Parser;
use dcmview::loader;
use dcmview::pixels;
use dcmview::server::{self, now_unix_ms, AppState, ServerConfig, TunnelConfig};
use dcmview::types;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "dcmview")]
#[command(about = "Ephemeral DICOM inspection server")]
struct Cli {
	#[arg(required = true)]
	paths: Vec<PathBuf>,

	#[arg(short = 'p', long = "port", default_value_t = 0)]
	port: u16,

	#[arg(long = "host", default_value = "127.0.0.1")]
	host: String,

	#[arg(long = "no-browser")]
	no_browser: bool,

	#[arg(long = "tunnel")]
	tunnel: bool,

	#[arg(long = "tunnel-host")]
	tunnel_host: Option<String>,

	#[arg(long = "tunnel-port", default_value_t = 0)]
	tunnel_port: u16,

	#[arg(long = "timeout")]
	timeout: Option<u64>,

	#[arg(long = "no-recursive")]
	no_recursive: bool,
}

#[tokio::main]
async fn main() {
	if let Err(error) = run().await {
		eprintln!("{error}");
		std::process::exit(1);
	}
}

async fn run() -> Result<()> {
	tracing_subscriber::fmt().with_env_filter("info,jpeg2k=warn").init();
	let cli = Cli::parse();

	let load_report = loader::discover(
		&cli.paths,
		loader::DiscoverOptions {
			recursive: !cli.no_recursive,
		},
	)
	.await
	.context("failed to discover DICOM files")?;

	print_load_summary(&load_report, &cli.paths);

	let tunnel = if cli.tunnel {
		let host = cli
			.tunnel_host
			.clone()
			.ok_or_else(|| anyhow::anyhow!("dcmview: --tunnel requires --tunnel-host"))?;
		Some(TunnelConfig {
			host,
			port: cli.tunnel_port,
		})
	} else {
		None
	};

	let state = AppState {
		files: Arc::new(load_report.files),
		pixel_cache: pixels::new_cache(),
		tag_cache: Arc::new(Mutex::new(HashMap::new())),
		tunnel_info: None,
		tunnel_handle: None,
		server_start: Instant::now(),
		server_start_ms: now_unix_ms(),
		last_request: Arc::new(AtomicU64::new(now_unix_ms())),
	};

	let run_result = server::run(
		ServerConfig {
			host: cli.host,
			port: cli.port,
			timeout_seconds: cli.timeout,
			open_browser: !cli.no_browser,
			tunnel,
		},
		state,
	)
	.await;

	match run_result {
		Ok(()) => Ok(()),
		Err(error) => {
			let message = error.to_string();
			if cli.port != 0 && (message.contains("Address already in use") || message.contains("failed to bind")) {
				Err(anyhow::anyhow!(
					"dcmview: port {} is already in use — try --port 0 for auto-assign",
					cli.port
				))
			} else {
				Err(error)
			}
		}
	}
}

fn print_load_summary(report: &types::LoadReport, input_paths: &[PathBuf]) {
	let recursive_note = if report.searched_recursive {
		"searched recursively"
	} else {
		"searched top-level only"
	};
	let path_label = if input_paths.len() == 1 {
		input_paths[0].display().to_string()
	} else {
		format!("{} path(s)", input_paths.len())
	};

	if report.skipped == 0 {
		if report.files.len() == 1 {
			println!("dcmview: loaded 1 DICOM file");
		} else {
			println!(
				"dcmview: loaded {} DICOM file(s) from {} ({})",
				report.files.len(),
				path_label,
				recursive_note
			);
		}
	} else {
		println!(
			"dcmview: loaded {} DICOM file(s) from {} ({} skipped — not valid DICOM, {})",
			report.files.len(),
			path_label,
			report.skipped,
			recursive_note
		);
	}
}
