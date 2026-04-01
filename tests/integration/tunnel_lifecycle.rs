use dcmview::tunnel;
use std::env;
use std::sync::Mutex;

static PATH_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn warns_and_continues_when_ssh_is_missing() {
	let _guard = PATH_LOCK.lock().expect("path lock");
	let original_path = env::var_os("PATH");
	env::set_var("PATH", "/definitely-not-a-real-bin");

	let runtime = tunnel::start_tunnel(4312, "user@example.com".to_string(), 0)
		.expect("tunnel start should gracefully fallback when ssh is missing");

	if let Some(path) = original_path {
		env::set_var("PATH", path);
	} else {
		env::remove_var("PATH");
	}

	assert!(runtime.handle.is_none(), "no process handle should be created without ssh");
	assert_eq!(runtime.info.tunnel_port, 4312);
	assert!(
		runtime.warning.unwrap_or_default().contains("ssh not found"),
		"warning text should explain missing ssh"
	);
}


#[test]
fn rejects_option_like_tunnel_host_values() {
	let error = match tunnel::start_tunnel(4312, "-oProxyCommand=echo-pwned".to_string(), 0) {
		Ok(_) => panic!("option-like tunnel host should be rejected"),
		Err(error) => error,
	};
	assert!(
		error.to_string().contains("must not start with '-'"),
		"error should explain why the host value is rejected"
	);
}