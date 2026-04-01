use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
	// Emit cargo:rerun-if-changed for every file in frontend/src/ recursively,
	// plus the config and lock files. Cargo's directory-level watch does not
	// recurse into subdirectories, so we walk the tree ourselves.
	emit_src_fingerprints(Path::new("frontend/src"));
	println!("cargo:rerun-if-changed=frontend/package.json");
	println!("cargo:rerun-if-changed=frontend/package-lock.json");
	println!("cargo:rerun-if-changed=frontend/svelte.config.js");
	println!("cargo:rerun-if-changed=frontend/vite.config.ts");
	println!("cargo:rerun-if-env-changed=DCMVIEW_NODE_PATH");
	println!("cargo:rerun-if-env-changed=DCMVIEW_NPM_PATH");

	let node_bin = match resolve_tool_path("node", "DCMVIEW_NODE_PATH") {
		Ok(path) => path,
		Err(error) => fatal(&error),
	};
	let npm_bin = match resolve_tool_path("npm", "DCMVIEW_NPM_PATH") {
		Ok(path) => path,
		Err(error) => fatal(&error),
	};

	if !tool_exists(&node_bin) || !tool_exists(&npm_bin) {
		fatal("Node.js and npm are required to build dcmview");
	}

	// Only run `npm ci` when package-lock.json content has changed since the last
	// successful install. Persist a SHA-256 digest in OUT_DIR.
	if needs_npm_install() {
		run_npm(&npm_bin, ["ci"]);
		write_install_stamp();
	}

	run_npm(&npm_bin, ["run", "build"]);
}

fn fatal(message: &str) -> ! {
	println!("cargo:error={message}");
	std::process::exit(1);
}

// ---------------------------------------------------------------------------
// Fingerprinting helpers
// ---------------------------------------------------------------------------

/// Walk `dir` recursively and emit `cargo:rerun-if-changed` for every file.
fn emit_src_fingerprints(dir: &Path) {
	let entries = match std::fs::read_dir(dir) {
		Ok(e) => e,
		Err(_) => return,
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.is_dir() {
			emit_src_fingerprints(&path);
		} else {
			println!("cargo:rerun-if-changed={}", path.display());
		}
	}
}

/// Returns true when package-lock.json has changed since the last npm ci run.
fn needs_npm_install() -> bool {
	let stamp_path = stamp_file_path();
	let current = lock_fingerprint();
	match std::fs::read_to_string(&stamp_path) {
		Ok(saved) => saved.trim() != current.trim(),
		Err(_) => true, // no stamp yet — first build or OUT_DIR was cleaned
	}
}

/// Persist the current package-lock.json digest so the next build can skip
/// `npm ci` if nothing changed.
fn write_install_stamp() {
	if std::env::var("OUT_DIR").is_ok() {
		let _ = std::fs::write(stamp_file_path(), lock_fingerprint());
	}
}

/// Returns the path to the npm-ci stamp file inside Cargo's OUT_DIR.
fn stamp_file_path() -> PathBuf {
	let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
	PathBuf::from(out_dir).join("npm-ci.stamp")
}

/// SHA-256 digest of package-lock.json content.
fn lock_fingerprint() -> String {
	let bytes = std::fs::read("frontend/package-lock.json")
		.expect("frontend/package-lock.json must exist");
	let digest = Sha256::digest(bytes);
	format!("{digest:x}")
}

// ---------------------------------------------------------------------------
// npm helpers
// ---------------------------------------------------------------------------

fn resolve_tool_path(default_tool: &str, env_var: &str) -> Result<String, String> {
	if let Ok(raw) = std::env::var(env_var) {
		let configured = raw.trim();
		if configured.is_empty() {
			return Err(format!("{env_var} is set but empty"));
		}
		let candidate = PathBuf::from(configured);
		if !candidate.is_absolute() {
			return Err(format!("{env_var} must be an absolute path when provided"));
		}
		return Ok(candidate.to_string_lossy().to_string());
	}
	Ok(default_tool.to_string())
}

fn tool_exists(tool: &str) -> bool {
	Command::new(tool)
		.arg("--version")
		.output()
		.map(|output| output.status.success())
		.unwrap_or(false)
}

fn run_npm<'a>(npm_bin: &str, args: impl IntoIterator<Item = &'a str>) {
	let status = Command::new(npm_bin)
		.args(args)
		.current_dir("frontend")
		.status()
		.expect("failed to run npm for frontend build");

	if !status.success() {
		panic!("frontend npm command failed with status: {status}");
	}
}