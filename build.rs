use std::process::Command;

fn main() {
	println!("cargo:rerun-if-changed=frontend/src");
	println!("cargo:rerun-if-changed=frontend/package.json");
	println!("cargo:rerun-if-changed=frontend/package-lock.json");
	println!("cargo:rerun-if-changed=frontend/svelte.config.js");
	println!("cargo:rerun-if-changed=frontend/vite.config.ts");

	if !tool_exists("node") || !tool_exists("npm") {
		println!("cargo:error=Node.js and npm are required to build dcmview");
		std::process::exit(1);
	}

	run_npm(["ci"]);
	run_npm(["run", "build"]);
}

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
