use crate::types::TunnelInfo;
use anyhow::{anyhow, Context, Result};
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct TunnelHandle {
	child: Arc<Mutex<Child>>,
}

impl TunnelHandle {
	pub fn shutdown(&self) {
		if let Ok(mut child) = self.child.lock() {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

pub struct TunnelRuntime {
	pub info: TunnelInfo,
	pub handle: Option<TunnelHandle>,
	pub warning: Option<String>,
}

pub fn start_tunnel(bind_port: u16, tunnel_host: String, tunnel_port: u16) -> Result<TunnelRuntime> {
	let expose_port = if tunnel_port == 0 { bind_port } else { tunnel_port };

	if !ssh_available() {
		return Ok(TunnelRuntime {
			info: TunnelInfo {
				tunnel_host,
				tunnel_port: expose_port,
			},
			handle: None,
			warning: Some("dcmview: warning — ssh not found on PATH, cannot establish tunnel".to_string()),
		});
	}

	let mut child = Command::new("ssh")
		.args([
			"-N",
			"-o",
			"ExitOnForwardFailure=yes",
			"-o",
			"ServerAliveInterval=10",
			"-L",
			&format!("{expose_port}:127.0.0.1:{bind_port}"),
			&tunnel_host,
		])
		.stderr(Stdio::piped())
		.spawn()
		.context("failed to spawn ssh tunnel")?;

	if let Some(stderr) = child.stderr.take() {
		thread::spawn(move || {
			let reader = BufReader::new(stderr);
			for line in reader.lines().map_while(Result::ok) {
				eprintln!("dcmview: tunnel warning: {line}");
			}
		});
	}

	wait_for_local_port(expose_port).context("tunnel readiness probe failed")?;

	let handle = TunnelHandle {
		child: Arc::new(Mutex::new(child)),
	};

	Ok(TunnelRuntime {
		info: TunnelInfo {
			tunnel_host,
			tunnel_port: expose_port,
		},
		handle: Some(handle),
		warning: None,
	})
}

fn ssh_available() -> bool {
	Command::new("ssh")
		.arg("-V")
		.output()
		.map(|output| output.status.success() || !output.stderr.is_empty())
		.unwrap_or(false)
}

fn wait_for_local_port(port: u16) -> Result<()> {
	let deadline = Instant::now() + Duration::from_secs(5);
	while Instant::now() < deadline {
		if TcpStream::connect(("127.0.0.1", port)).is_ok() {
			return Ok(());
		}
		thread::sleep(Duration::from_millis(100));
	}
	Err(anyhow!("timeout waiting for tunnel readiness"))
}
