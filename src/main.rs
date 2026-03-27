use anyhow::{Context, Result};
use clap::Parser;
use dcmview::loader;
use dcmview::pixels;
use dcmview::server::{self, now_unix_ms, AppState, ServerConfig};
use dcmview::tunnel;
use dcmview::types;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
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
	tracing_subscriber::fmt().with_env_filter("info").init();
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

	let mut tunnel_info = None;
	let mut tunnel_handle = None;
	if cli.tunnel {
		let tunnel_host = cli
			.tunnel_host
			.clone()
			.ok_or_else(|| anyhow::anyhow!("dcmview: --tunnel requires --tunnel-host"))?;
		let runtime = tunnel::start_tunnel(cli.port, tunnel_host, cli.tunnel_port)?;
		if let Some(warning) = runtime.warning.as_deref() {
			eprintln!("{warning}");
			eprintln!("dcmview: to forward manually, run on your local machine:");
			eprintln!(
				"dcmview:   ssh -L {0}:localhost:{0} {1}",
				runtime.info.tunnel_port, runtime.info.tunnel_host
			);
		}
		tunnel_info = Some(Arc::new(runtime.info));
		tunnel_handle = runtime.handle.map(Arc::new);
	}

	let state = AppState {
		files: Arc::new(load_report.files),
		pixel_cache: pixels::new_cache(),
		tunnel_info,
		tunnel_handle,
		server_start: Instant::now(),
		server_start_ms: now_unix_ms(),
		last_request: Arc::new(AtomicU64::new(now_unix_ms())),
	};

	server::run(
		ServerConfig {
			host: cli.host,
			port: cli.port,
			timeout_seconds: cli.timeout,
		},
		state,
	)
	.await
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
