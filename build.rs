use std::path::Path;
use std::process::Command;

fn main() {
	// Emit cargo:rerun-if-changed for every file in frontend/src/ recursively,
	// plus the config and lock files.  Cargo's directory-level watch does not
	// recurse into subdirectories, so we walk the tree ourselves.
	emit_src_fingerprints(Path::new("frontend/src"));
	println!("cargo:rerun-if-changed=frontend/package.json");
	println!("cargo:rerun-if-changed=frontend/package-lock.json");
	println!("cargo:rerun-if-changed=frontend/svelte.config.js");
	println!("cargo:rerun-if-changed=frontend/vite.config.ts");

	if !tool_exists("node") || !tool_exists("npm") {
		println!("cargo:error=Node.js and npm are required to build dcmview");
		std::process::exit(1);
	}

	// Only run `npm ci` when package-lock.json has changed since the last
	// successful install.  We persist a stamp file in OUT_DIR whose content
	// is a fingerprint (size + mtime) of package-lock.json.
	if needs_npm_install() {
		run_npm(["ci"]);
		write_install_stamp();
	}

	run_npm(["run", "build"]);
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

/// Persist the current package-lock.json fingerprint so the next build can
/// skip `npm ci` if nothing changed.
fn write_install_stamp() {
	if let Ok(out_dir) = std::env::var("OUT_DIR") {
		let _ = std::fs::write(stamp_file_path(), lock_fingerprint());
		let _ = out_dir; // suppress unused-variable lint on older toolchains
	}
}

/// Returns the path to the npm-ci stamp file inside Cargo's OUT_DIR.
fn stamp_file_path() -> std::path::PathBuf {
	let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
	std::path::PathBuf::from(out_dir).join("npm-ci.stamp")
}

/// A cheap fingerprint for package-lock.json: "size-mtime_secs".
/// Changing any byte changes the file size or mtime, so this is reliable.
fn lock_fingerprint() -> String {
	let meta = std::fs::metadata("frontend/package-lock.json")
		.expect("frontend/package-lock.json must exist");
	let mtime = meta
		.modified()
		.ok()
		.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
		.map(|d| d.as_secs())
		.unwrap_or(0);
	format!("{}-{}", meta.len(), mtime)
}

// ---------------------------------------------------------------------------
// npm helpers
// ---------------------------------------------------------------------------

fn tool_exists(tool: &str) -> bool {
	Command::new(tool)
		.arg("--version")
		.output()
		.map(|output| output.status.success())
		.unwrap_or(false)
}

fn run_npm(args: impl IntoIterator<Item = &'static str>) {
	let status = Command::new("npm")
		.args(args)
		.current_dir("frontend")
		.status()
		.expect("failed to run npm for frontend build");

	if !status.success() {
		panic!("frontend npm command failed with status: {status}");
	}
}
