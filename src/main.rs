use anyhow::{Context, Result};
use clap::Parser;
use dcmview::annotations;
use dcmview::loader;
use dcmview::server::{self, now_unix_ms, AppState, ServerConfig, TunnelConfig};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

const VSCODE_BRIDGE_URL_ENV: &str = "DCMVIEW_VSCODE_BRIDGE_URL";
const VSCODE_BRIDGE_TOKEN_ENV: &str = "DCMVIEW_VSCODE_BRIDGE_TOKEN";
const VSCODE_BRIDGE_BYPASS_ENV: &str = "DCMVIEW_VSCODE_BYPASS";
const VSCODE_BRIDGE_REGISTRY_DIR_ENV: &str = "DCMVIEW_VSCODE_BRIDGE_REGISTRY_DIR";
const VSCODE_BRIDGE_DEBUG_ENV: &str = "DCMVIEW_VSCODE_BRIDGE_DEBUG";
const BRIDGE_REGISTRY_MAX_AGE_MS: u64 = 3 * 60 * 60 * 1000;
const DISCOVERY_EVENT_CAPACITY: usize = 64;

#[derive(Debug, Parser)]
#[command(
    name = "dcmview",
    version,
    about = "Start a temporary local DICOM inspection viewer",
    long_about = "Start a temporary local web server for inspecting DICOM files, directories, image frames, tags, and optional ROI annotations. dcmview is intended for research and development inspection, not clinical diagnosis.",
    after_long_help = "\
Examples:
  dcmview ./scan.dcm
  dcmview ./study_dir
  dcmview --no-recursive ./study_dir
  dcmview --no-browser --host 127.0.0.1 --port 8010 ./study_dir
  ssh -L 8010:127.0.0.1:8010 user@remote
  dcmview --annotations ./rois.csv ./study_dir
  dcmview --filter Modality=CT --filter PatientID=phantom ./study_dir

For remote use, run dcmview on the machine that has the DICOM files, keep the
server bound to 127.0.0.1, and forward the chosen port over SSH."
)]
struct Cli {
    #[arg(
        value_name = "PATH",
        required_unless_present = "vscode_bridge_client",
        help = "DICOM file or directory to inspect; repeat for multiple inputs"
    )]
    paths: Vec<PathBuf>,

    #[arg(
        short = 'p',
        long = "port",
        value_name = "PORT",
        default_value_t = 0,
        help = "Local HTTP port to bind; 0 selects an available port"
    )]
    port: u16,

    #[arg(
        long = "host",
        value_name = "ADDR",
        default_value = "127.0.0.1",
        help = "Local interface to bind; keep 127.0.0.1 unless you understand the network exposure"
    )]
    host: String,

    #[arg(
        long = "no-browser",
        help = "Print the viewer URL instead of opening a browser automatically"
    )]
    no_browser: bool,

    #[arg(
        long = "tunnel",
        help = "Start an SSH local port-forward helper after the viewer starts"
    )]
    tunnel: bool,

    #[arg(
        long = "tunnel-host",
        value_name = "SSH_HOST",
        help = "SSH host used with --tunnel, for example user@example.org"
    )]
    tunnel_host: Option<String>,

    #[arg(
        long = "tunnel-port",
        value_name = "PORT",
        default_value_t = 0,
        help = "Local forwarded port for --tunnel; 0 reuses the viewer port"
    )]
    tunnel_port: u16,

    #[arg(
        long = "timeout",
        value_name = "SECONDS",
        help = "Exit after this many seconds without API or browser requests"
    )]
    timeout: Option<u64>,

    #[arg(
        long = "no-recursive",
        help = "Scan only the top level of input directories"
    )]
    no_recursive: bool,

    #[arg(
        long = "annotations",
        value_name = "CSV",
        help = "Load EMBED-style ROI annotations from CSV without modifying the file"
    )]
    annotations: Option<PathBuf>,

    #[arg(
        long = "filter",
        value_name = "FIELD=VALUE",
        value_parser = parse_scan_filter,
        help = "Include only files whose metadata field contains the value; repeatable"
    )]
    filters: Vec<loader::ScanFilter>,

    #[arg(
        long = "startup-json",
        hide = true,
        help = "Print machine-readable startup events for integrations"
    )]
    startup_json: bool,

    #[arg(
        long = "vscode-bridge-client",
        hide = true,
        num_args = 1..,
        allow_hyphen_values = true
    )]
    vscode_bridge_client: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeLaunchRequest {
    program: String,
    args: Vec<String>,
    cwd: String,
    wait: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    binary_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeLaunchResponse {
    session_id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeWaitResponse {
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeEndpoint {
    url: String,
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeRegistryEntry {
    bridge_url: String,
    token: String,
    workspace_roots: Option<Vec<String>>,
    created_at_ms: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
enum BridgeLaunchError {
    #[error("failed to contact VS Code bridge: {0}")]
    Connect(String),
    #[error("VS Code bridge returned {status}: {message}")]
    Http {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("failed to parse VS Code bridge launch response: {0}")]
    Decode(String),
    #[error("VS Code bridge request failed: {0}")]
    Request(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryMatch {
    AllowAny,
    RequireWorkspace,
}

fn parse_scan_filter(raw: &str) -> std::result::Result<loader::ScanFilter, String> {
    raw.parse()
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(exit_code) => {
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<i32> {
    let program_name = env::args().next().unwrap_or_else(|| "dcmview".to_string());
    let raw_args = env::args().skip(1).collect::<Vec<_>>();
    let cli = Cli::parse_from(std::iter::once(program_name).chain(raw_args.clone()));

    if let Some(bridge_args) = cli.vscode_bridge_client {
        return run_vscode_bridge_client(bridge_args).await;
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let bridge_endpoints = discover_vscode_bridge_endpoints(&cwd, RegistryMatch::RequireWorkspace);
    if !bridge_endpoints.is_empty() {
        match run_vscode_bridge_launch("dcmview", &raw_args, &bridge_endpoints).await {
            Ok(exit_code) => return Ok(exit_code),
            Err(error) => {
                eprintln!(
                    "dcmview: VS Code bridge unavailable ({error}); falling back to local viewer"
                );
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter("info,jpeg2k=warn")
        .init();

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

    let registry = server::FileRegistry::new();
    let annotation_store = annotations::AnnotationStore::empty();
    let annotation_source = cli
        .annotations
        .as_ref()
        .map(|path| {
            annotations::AnnotationSource::from_path(path)
                .with_context(|| format!("failed to load annotations from {}", path.display()))
                .map(Arc::new)
        })
        .transpose()?;
    let shutdown_notify = Arc::new(Notify::new());
    let exit_code = Arc::new(AtomicI32::new(0));

    let state = AppState::new(registry.clone(), annotation_store.clone());

    ProgressiveDiscovery {
        input_paths: cli.paths.clone(),
        recursive: !cli.no_recursive,
        filters: cli.filters.clone(),
        annotation_source,
        registry,
        annotation_store,
        shutdown_notify: shutdown_notify.clone(),
        exit_code: exit_code.clone(),
    }
    .spawn();

    let run_result = server::run(
        ServerConfig {
            host: cli.host,
            port: cli.port,
            timeout_seconds: cli.timeout,
            open_browser: !cli.no_browser,
            startup_json: cli.startup_json,
            tunnel,
            shutdown: Some(shutdown_notify),
        },
        state,
    )
    .await;

    match run_result {
        Ok(()) => Ok(exit_code.load(Ordering::Relaxed)),
        Err(error) => {
            let message = error.to_string();
            if cli.port != 0
                && (message.contains("Address already in use")
                    || message.contains("failed to bind"))
            {
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

async fn run_vscode_bridge_client(values: Vec<String>) -> Result<i32> {
    let Some((program, args)) = values.split_first() else {
        return Ok(1);
    };

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let bridge_endpoints = discover_vscode_bridge_endpoints(&cwd, RegistryMatch::AllowAny);
    if bridge_endpoints.is_empty() {
        return fallback_to_local_viewer(args);
    }

    match run_vscode_bridge_launch(program, args, &bridge_endpoints).await {
        Ok(exit_code) => Ok(exit_code),
        Err(error) => {
            eprintln!(
                "dcmview: VS Code bridge unavailable ({error}); falling back to local viewer"
            );
            fallback_to_local_viewer(args)
        }
    }
}

async fn run_vscode_bridge_launch(
    program: &str,
    args: &[String],
    bridge_endpoints: &[BridgeEndpoint],
) -> Result<i32> {
    let client = reqwest::Client::builder()
        .build()
        .context("failed to create VS Code bridge HTTP client")?;
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let registry_endpoints =
        discover_vscode_bridge_registry_endpoints(&cwd, RegistryMatch::AllowAny, now_unix_ms());
    let launch = BridgeLaunchRequest {
        program: program.to_string(),
        args: args.to_vec(),
        cwd: cwd.display().to_string(),
        wait: false,
        binary_path: env::current_exe()
            .ok()
            .map(|path| path.display().to_string()),
    };

    let mut last_error = None;
    for endpoint in bridge_endpoints {
        match launch_vscode_session(&client, endpoint, &launch).await {
            Ok(launch_response) => {
                return match wait_for_launched_vscode_session(&client, endpoint, launch_response)
                    .await
                {
                    Ok(exit_code) => Ok(exit_code),
                    Err(error) => {
                        eprintln!(
                            "dcmview: VS Code bridge session was captured but wait failed: {error}"
                        );
                        Ok(1)
                    }
                };
            }
            Err(error) => {
                if should_remove_registry_entry_after_launch_error(
                    &error,
                    endpoint,
                    &registry_endpoints,
                ) {
                    remove_vscode_bridge_registry_endpoint(endpoint);
                }
                bridge_debug(&format!("endpoint {} failed: {error}", endpoint.url));
                last_error = Some(anyhow::Error::from(error));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no VS Code bridge endpoints available")))
}

async fn launch_vscode_session(
    client: &reqwest::Client,
    endpoint: &BridgeEndpoint,
    launch: &BridgeLaunchRequest,
) -> Result<BridgeLaunchResponse, BridgeLaunchError> {
    let launch_url = format!("{}/launch", endpoint.url.trim_end_matches('/'));
    let response = match client
        .post(launch_url)
        .bearer_auth(&endpoint.token)
        .json(launch)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => {
            return Err(BridgeLaunchError::Connect(error.to_string()));
        }
        Err(error) => return Err(BridgeLaunchError::Request(error.to_string())),
    };
    if !response.status().is_success() {
        let status = response.status();
        let message = bridge_error_response_message(response).await;
        return Err(BridgeLaunchError::Http { status, message });
    }
    response
        .json::<BridgeLaunchResponse>()
        .await
        .map_err(|error| BridgeLaunchError::Decode(error.to_string()))
}

async fn bridge_error_response_message(response: reqwest::Response) -> String {
    let status = response.status();
    let Ok(text) = response.text().await else {
        return status.to_string();
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(error) = value.get("error").and_then(|value| value.as_str()) {
            return error.to_string();
        }
    }
    if text.is_empty() {
        status.to_string()
    } else {
        text
    }
}

fn should_remove_registry_entry_after_launch_error(
    error: &BridgeLaunchError,
    endpoint: &BridgeEndpoint,
    registry_endpoints: &[BridgeEndpoint],
) -> bool {
    matches!(error, BridgeLaunchError::Connect(_)) && registry_endpoints.contains(endpoint)
}

async fn wait_for_launched_vscode_session(
    client: &reqwest::Client,
    endpoint: &BridgeEndpoint,
    launch_response: BridgeLaunchResponse,
) -> Result<i32> {
    println!("dcmview: opened in VS Code at {}", launch_response.url);
    let wait_url = format!(
        "{}/sessions/{}/wait",
        endpoint.url.trim_end_matches('/'),
        launch_response.session_id
    );
    let stop_url = format!(
        "{}/sessions/{}/stop",
        endpoint.url.trim_end_matches('/'),
        launch_response.session_id
    );
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(windows)]
    let ctrl_break = async {
        let mut signal =
            tokio::signal::windows::ctrl_break().context("failed to listen for Ctrl+Break")?;
        signal.recv().await;
        Ok::<(), anyhow::Error>(())
    };

    #[cfg(not(windows))]
    let ctrl_break = std::future::pending::<Result<()>>();

    tokio::select! {
        wait_result = wait_for_vscode_session(client, &wait_url, &endpoint.token) => wait_result,
        signal_result = ctrl_c => {
            signal_result.context("failed to listen for Ctrl+C")?;
            let _ = client
                .post(stop_url)
                .bearer_auth(&endpoint.token)
                .timeout(Duration::from_secs(5))
                .send()
                .await;
            Ok(130)
        },
        signal_result = ctrl_break => {
            signal_result?;
            let _ = client
                .post(stop_url)
                .bearer_auth(&endpoint.token)
                .timeout(Duration::from_secs(5))
                .send()
                .await;
            Ok(130)
        }
    }
}

fn discover_vscode_bridge_endpoints(
    cwd: &Path,
    registry_match: RegistryMatch,
) -> Vec<BridgeEndpoint> {
    if env::var(VSCODE_BRIDGE_BYPASS_ENV).as_deref() == Ok("1") {
        bridge_debug("bridge discovery bypassed by DCMVIEW_VSCODE_BYPASS=1");
        return Vec::new();
    }

    let mut endpoints = Vec::new();
    if let (Ok(url), Ok(token)) = (
        env::var(VSCODE_BRIDGE_URL_ENV),
        env::var(VSCODE_BRIDGE_TOKEN_ENV),
    ) {
        if !url.is_empty() && !token.is_empty() {
            bridge_debug(&format!("accepted env endpoint {url}"));
            endpoints.push(BridgeEndpoint { url, token });
        }
    }

    for endpoint in discover_vscode_bridge_registry_endpoints(cwd, registry_match, now_unix_ms()) {
        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }
    bridge_debug(&format!(
        "discovered {} bridge endpoint(s)",
        endpoints.len()
    ));
    endpoints
}

fn discover_vscode_bridge_registry_endpoints(
    cwd: &Path,
    registry_match: RegistryMatch,
    now_ms: u64,
) -> Vec<BridgeEndpoint> {
    let mut endpoints = Vec::new();
    for registry_dir in vscode_bridge_registry_dirs() {
        for endpoint in discover_vscode_bridge_registry_endpoints_from_dir(
            cwd,
            registry_match,
            now_ms,
            &registry_dir,
        ) {
            if !endpoints.contains(&endpoint) {
                endpoints.push(endpoint);
            }
        }
    }
    endpoints
}

fn discover_vscode_bridge_registry_endpoints_from_dir(
    cwd: &Path,
    registry_match: RegistryMatch,
    now_ms: u64,
    registry_dir: &Path,
) -> Vec<BridgeEndpoint> {
    bridge_debug(&format!(
        "scanning bridge registry dir {}",
        registry_dir.display()
    ));
    if !registry_dir_is_trusted(registry_dir) {
        bridge_debug(&format!(
            "registry dir untrusted or missing: {}",
            registry_dir.display()
        ));
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(registry_dir) else {
        bridge_debug(&format!(
            "registry dir unreadable: {}",
            registry_dir.display()
        ));
        return Vec::new();
    };
    let cwd = normalized_path(cwd);
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
            continue;
        };
        if !is_supported_registry_version(&value) {
            bridge_debug(&format!(
                "registry entry skipped unsupported version: {}",
                path.display()
            ));
            continue;
        }
        let Ok(registry) = serde_json::from_value::<BridgeRegistryEntry>(value) else {
            let _ = fs::remove_file(&path);
            bridge_debug(&format!(
                "registry entry removed malformed v1: {}",
                path.display()
            ));
            continue;
        };
        if registry.bridge_url.is_empty() || registry.token.is_empty() {
            bridge_debug(&format!(
                "registry entry skipped missing endpoint: {}",
                path.display()
            ));
            continue;
        }
        let Some(created_at) = registry.created_at_ms else {
            let _ = fs::remove_file(&path);
            bridge_debug(&format!(
                "registry entry removed missing timestamp: {}",
                path.display()
            ));
            continue;
        };
        if is_expired_registry_entry(created_at, now_ms) {
            let _ = fs::remove_file(&path);
            bridge_debug(&format!(
                "registry entry removed expired: {}",
                path.display()
            ));
            continue;
        }
        let match_score =
            workspace_match_score(&cwd, registry.workspace_roots.as_deref().unwrap_or(&[]));
        if registry_match == RegistryMatch::RequireWorkspace && match_score == 0 {
            bridge_debug(&format!(
                "registry entry skipped workspace mismatch: {}",
                path.display()
            ));
            continue;
        }
        bridge_debug(&format!("registry entry accepted: {}", path.display()));
        candidates.push((
            match_score,
            created_at,
            BridgeEndpoint {
                url: registry.bridge_url,
                token: registry.token,
            },
        ));
    }
    candidates.sort_by(|left, right| (right.0, right.1).cmp(&(left.0, left.1)));

    let mut endpoints = Vec::new();
    for (_, _, endpoint) in candidates {
        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }
    endpoints
}

fn is_supported_registry_version(value: &serde_json::Value) -> bool {
    match value.get("version") {
        None => true,
        Some(serde_json::Value::Number(number)) => number.as_u64() == Some(1),
        Some(_) => false,
    }
}

fn registry_dir_is_trusted(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    registry_metadata_is_trusted(&metadata)
}

#[cfg(unix)]
fn registry_metadata_is_trusted(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    registry_ownership_is_trusted(metadata.uid(), metadata.mode(), current_euid())
}

#[cfg(not(unix))]
fn registry_metadata_is_trusted(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn current_euid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn registry_ownership_is_trusted(uid: u32, mode: u32, euid: u32) -> bool {
    uid == euid && mode & 0o022 == 0
}

fn is_expired_registry_entry(created_at_ms: u64, now_ms: u64) -> bool {
    created_at_ms == 0
        || created_at_ms > now_ms.saturating_add(BRIDGE_REGISTRY_MAX_AGE_MS)
        || now_ms.saturating_sub(created_at_ms) > BRIDGE_REGISTRY_MAX_AGE_MS
}

fn remove_vscode_bridge_registry_endpoint(endpoint: &BridgeEndpoint) {
    for registry_dir in vscode_bridge_registry_dirs() {
        remove_vscode_bridge_registry_endpoint_from_dir(endpoint, &registry_dir);
    }
}

fn remove_vscode_bridge_registry_endpoint_from_dir(endpoint: &BridgeEndpoint, registry_dir: &Path) {
    if !registry_dir_is_trusted(registry_dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(registry_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(registry) = serde_json::from_str::<BridgeRegistryEntry>(&contents) else {
            continue;
        };
        if registry.bridge_url == endpoint.url && registry.token == endpoint.token {
            let _ = fs::remove_file(path);
        }
    }
}

fn vscode_bridge_registry_dirs() -> Vec<PathBuf> {
    if let Ok(configured) = env::var(VSCODE_BRIDGE_REGISTRY_DIR_ENV) {
        if !configured.is_empty() {
            return vec![PathBuf::from(configured)];
        }
    }
    let mut dirs = vec![vscode_bridge_registry_dir_from_values(
        None,
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("USERPROFILE").ok().as_deref(),
    )];
    dirs.extend(legacy_vscode_bridge_registry_dirs_from_values(
        env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        env::var("USER")
            .or_else(|_| env::var("USERNAME"))
            .ok()
            .as_deref(),
        &env::temp_dir(),
    ));
    dedupe_paths(dirs)
}

fn vscode_bridge_registry_dir_from_values(
    configured: Option<&str>,
    state_home: Option<&str>,
    home: Option<&str>,
    user_profile: Option<&str>,
) -> PathBuf {
    if let Some(configured) = configured {
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
    }

    if let Some(state_home) = state_home {
        if registry_env_path_is_absolute(state_home) {
            let state_home = PathBuf::from(state_home);
            return state_home.join("dcmview").join("vscode-bridges");
        }
    }

    if let Some(home) = home {
        if registry_env_path_is_absolute(home) {
            let home = PathBuf::from(home);
            return home
                .join(".local")
                .join("state")
                .join("dcmview")
                .join("vscode-bridges");
        }
    }

    if let Some(user_profile) = user_profile {
        if registry_env_path_is_absolute(user_profile) {
            let user_profile = PathBuf::from(user_profile);
            return user_profile
                .join(".local")
                .join("state")
                .join("dcmview")
                .join("vscode-bridges");
        }
    }

    PathBuf::from(".")
        .join(".local")
        .join("state")
        .join("dcmview")
        .join("vscode-bridges")
}

fn legacy_vscode_bridge_registry_dirs_from_values(
    runtime_dir: Option<&str>,
    user: Option<&str>,
    tmp_dir: &Path,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(runtime_dir) = runtime_dir {
        if registry_env_path_is_absolute(runtime_dir) {
            let runtime_dir = PathBuf::from(runtime_dir);
            dirs.push(runtime_dir.join("dcmview").join("vscode-bridges"));
        }
    }

    let user = user.unwrap_or("default");
    dirs.push(tmp_dir.join(format!(
        "dcmview-vscode-bridges-{}",
        safe_registry_segment(user)
    )));
    dirs
}

fn registry_env_path_is_absolute(path: &str) -> bool {
    Path::new(path).is_absolute()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.as_bytes().get(1) == Some(&b':')
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in paths {
        if !result.contains(&path) {
            result.push(path);
        }
    }
    result
}

fn bridge_debug(message: &str) {
    if env::var(VSCODE_BRIDGE_DEBUG_ENV).as_deref() == Ok("1") {
        eprintln!("dcmview bridge: {message}");
    }
}

fn safe_registry_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn normalized_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn workspace_match_score(cwd: &Path, workspace_roots: &[String]) -> usize {
    workspace_roots
        .iter()
        .filter_map(|root| {
            let root = normalized_path(Path::new(root));
            cwd.strip_prefix(&root).ok().map(|_| root.as_os_str().len())
        })
        .max()
        .unwrap_or(0)
}

async fn wait_for_vscode_session(
    client: &reqwest::Client,
    wait_url: &str,
    token: &str,
) -> Result<i32> {
    let response = client
        .get(wait_url)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to wait for VS Code dcmview session")?;
    if !response.status().is_success() {
        return Ok(1);
    }
    let wait_response = response.json::<BridgeWaitResponse>().await?;
    Ok(wait_response.exit_code.unwrap_or(0))
}

fn fallback_to_local_viewer(args: &[String]) -> Result<i32> {
    let status = Command::new(env::current_exe().context("failed to resolve current executable")?)
        .args(args)
        .env(VSCODE_BRIDGE_BYPASS_ENV, "1")
        .status()
        .context("failed to run local dcmview fallback")?;
    Ok(status.code().unwrap_or(1))
}

struct ProgressiveDiscovery {
    input_paths: Vec<PathBuf>,
    recursive: bool,
    filters: Vec<loader::ScanFilter>,
    annotation_source: Option<Arc<annotations::AnnotationSource>>,
    registry: server::FileRegistry,
    annotation_store: annotations::AnnotationStore,
    shutdown_notify: Arc<Notify>,
    exit_code: Arc<AtomicI32>,
}

impl ProgressiveDiscovery {
    fn spawn(self) {
        tokio::spawn(async move {
            let Self {
                input_paths,
                recursive,
                filters,
                annotation_source,
                registry,
                annotation_store,
                shutdown_notify,
                exit_code,
            } = self;
            let (events_tx, mut events_rx) = mpsc::channel(DISCOVERY_EVENT_CAPACITY);
            let scan_paths = input_paths.clone();
            let filters_for_message = filters.clone();
            let scan = tokio::spawn(async move {
                loader::discover_progressive(
                    &scan_paths,
                    loader::DiscoverOptions { recursive, filters },
                    events_tx,
                )
                .await
            });

            while let Some(event) = events_rx.recv().await {
                match event {
                    loader::DiscoveryEvent::File(file) => {
                        registry.record_scanned();
                        let annotations = if let Some(source) = annotation_source.as_ref() {
                            match source.annotations_for_file(&file) {
                                Ok(annotations) => annotations,
                                Err(error) => {
                                    eprintln!("{error:#}");
                                    exit_code.store(1, Ordering::Relaxed);
                                    shutdown_notify.notify_one();
                                    return;
                                }
                            }
                        } else {
                            None
                        };
                        let index = registry.insert(*file);
                        if let Some(annotations) = annotations {
                            if let Err(error) =
                                annotation_store.insert_csv_if_unedited(index, annotations)
                            {
                                eprintln!("{error:#}");
                                exit_code.store(1, Ordering::Relaxed);
                                shutdown_notify.notify_one();
                                return;
                            }
                        }
                    }
                    loader::DiscoveryEvent::Skipped => {
                        registry.record_skipped();
                    }
                    loader::DiscoveryEvent::Filtered => {
                        registry.record_filtered();
                    }
                }
            }

            let scan_result = match scan.await {
                Ok(result) => result,
                Err(error) => Err(anyhow::anyhow!("loader worker panicked: {error}")),
            };

            registry.mark_scan_complete();
            match scan_result {
                Ok(report) => {
                    let files = registry.files_snapshot();
                    if files.is_empty() {
                        if report.filtered > 0 {
                            eprintln!(
                                "dcmview: no DICOM files matched active filters ({})",
                                format_scan_filters(&filters_for_message)
                            );
                        } else {
                            eprintln!("dcmview: no valid DICOM files found");
                        }
                        exit_code.store(1, Ordering::Relaxed);
                        shutdown_notify.notify_one();
                        return;
                    }

                    if let Some(source) = annotation_source.as_ref() {
                        let unmatched = source.unmatched_row_count(files.as_slice());
                        if unmatched > 0 {
                            eprintln!(
                            "dcmview: warning — {unmatched} annotation row(s) did not match discovered DICOM files"
                        );
                        }
                    }

                    print_progressive_load_summary(
                        files.len(),
                        report.skipped,
                        report.filtered,
                        report.searched_recursive,
                        &filters_for_message,
                        &input_paths,
                    );
                }
                Err(error) => {
                    eprintln!("failed to discover DICOM files: {error:#}");
                    exit_code.store(1, Ordering::Relaxed);
                    shutdown_notify.notify_one();
                }
            }
        });
    }
}

fn print_progressive_load_summary(
    file_count: usize,
    skipped: usize,
    filtered: usize,
    searched_recursive: bool,
    filters: &[loader::ScanFilter],
    input_paths: &[PathBuf],
) {
    let recursive_note = if searched_recursive {
        "searched recursively"
    } else {
        "searched top-level only"
    };
    let path_label = if input_paths.len() == 1 {
        input_paths[0].display().to_string()
    } else {
        format!("{} path(s)", input_paths.len())
    };

    let mut notes = Vec::new();
    if skipped > 0 {
        notes.push(format!("{skipped} skipped — not valid DICOM"));
    }
    if filtered > 0 {
        notes.push(format!("{filtered} filtered"));
    }
    if !filters.is_empty() {
        notes.push(format!("filters: {}", format_scan_filters(filters)));
    }
    notes.push(recursive_note.to_string());
    let note = notes.join(", ");

    if file_count == 1 && skipped == 0 && filtered == 0 && filters.is_empty() {
        println!("dcmview: loaded 1 DICOM file");
    } else {
        println!(
            "dcmview: loaded {} DICOM file(s) from {} ({})",
            file_count, path_label, note
        );
    }
}

fn format_scan_filters(filters: &[loader::ScanFilter]) -> String {
    filters
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::collections::HashSet;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn bridge_launch_request_serializes_camel_case_contract() {
        let request = BridgeLaunchRequest {
            program: "dcmview".to_string(),
            args: vec!["scan.dcm".to_string()],
            cwd: "/workspace".to_string(),
            wait: false,
            binary_path: None,
        };

        let value = serde_json::to_value(request).expect("bridge launch request serializes");

        assert_eq!(value["program"], "dcmview");
        assert_eq!(value["args"], serde_json::json!(["scan.dcm"]));
        assert_eq!(value["cwd"], "/workspace");
        assert_eq!(value["wait"], false);
        assert_eq!(value.get("binaryPath"), None);
    }

    #[test]
    fn launcher_cli_flags_exist_on_clap_contract() {
        let command = Cli::command();
        let flags = command
            .get_arguments()
            .filter_map(|argument| argument.get_long())
            .collect::<HashSet<_>>();
        let launcher_flags = launcher_long_flags();

        for expected in launcher_flags {
            assert!(
                flags.contains(expected.as_str()),
                "launcher-used CLI flag --{expected} must exist on Cli"
            );
        }
    }

    #[test]
    fn cli_definition_satisfies_clap_debug_assertions() {
        Cli::command().debug_assert();
    }

    fn launcher_long_flags() -> HashSet<String> {
        let mut flags = HashSet::new();
        collect_long_flags(include_str!("../python/dcmview_py/wrapper.py"), &mut flags);
        collect_long_flags(include_str!("../vscode/src/extension.ts"), &mut flags);
        flags
    }

    fn collect_long_flags(source: &str, flags: &mut HashSet<String>) {
        for segment in source.split("--").skip(1) {
            let flag = segment
                .chars()
                .take_while(|character| character.is_ascii_lowercase() || *character == '-')
                .collect::<String>();
            if !flag.is_empty() {
                flags.insert(flag);
            }
        }
    }

    #[test]
    fn bridge_launch_request_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../docs/contracts/bridge-protocol.json"))
                .expect("bridge fixture parses");
        let request: BridgeLaunchRequest =
            serde_json::from_value(fixture["launch"]["request"].clone())
                .expect("launch request fixture parses");

        let value = serde_json::to_value(request).expect("bridge launch request serializes");

        assert_eq!(fixture["launch"]["method"], "POST");
        assert_eq!(fixture["launch"]["path"], "/launch");
        assert_eq!(value, fixture["launch"]["request"]);
    }

    #[test]
    fn bridge_responses_parse_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../docs/contracts/bridge-protocol.json"))
                .expect("bridge fixture parses");

        let launch: BridgeLaunchResponse =
            serde_json::from_value(fixture["launch"]["response"].clone())
                .expect("launch response fixture parses");
        let wait: BridgeWaitResponse = serde_json::from_value(fixture["wait"]["response"].clone())
            .expect("wait response fixture parses");

        assert_eq!(launch.session_id, "session-1");
        assert_eq!(launch.url, "http://127.0.0.1:51234");
        assert_eq!(wait.exit_code, Some(0));
    }

    #[test]
    fn bridge_wait_response_deserializes_camel_case_contract() {
        let response: BridgeWaitResponse =
            serde_json::from_str(r#"{"exitCode":7}"#).expect("bridge wait response parses");

        assert_eq!(response.exit_code, Some(7));
    }

    #[test]
    fn bridge_registry_endpoints_prefer_matching_workspace_then_newest() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
            "USERPROFILE",
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let cwd = workspace.join("nested");
        let now_ms = now_unix_ms();
        fs::create_dir_all(&cwd).expect("workspace dirs");
        fs::write(temp.path().join("invalid.json"), "{").expect("invalid registry");
        fs::write(
            temp.path().join("old.json"),
            serde_json::json!({
                "version": 1,
                "instanceId": "old",
                "bridgeUrl": "http://127.0.0.1:1111",
                "token": "old-token",
                "workspaceRoots": [temp.path().join("elsewhere")],
                "createdAtMs": now_ms
            })
            .to_string(),
        )
        .expect("old registry");
        fs::write(
            temp.path().join("match.json"),
            serde_json::json!({
                "version": 1,
                "instanceId": "match",
                "bridgeUrl": "http://127.0.0.1:2222",
                "token": "match-token",
                "workspaceRoots": [workspace],
                "createdAtMs": now_ms.saturating_sub(1)
            })
            .to_string(),
        )
        .expect("matching registry");

        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        env::remove_var(VSCODE_BRIDGE_URL_ENV);
        env::remove_var(VSCODE_BRIDGE_TOKEN_ENV);
        env::remove_var(VSCODE_BRIDGE_BYPASS_ENV);
        let endpoints = discover_vscode_bridge_endpoints(&cwd, RegistryMatch::AllowAny);

        assert_eq!(
            endpoints,
            vec![
                BridgeEndpoint {
                    url: "http://127.0.0.1:2222".to_string(),
                    token: "match-token".to_string(),
                },
                BridgeEndpoint {
                    url: "http://127.0.0.1:1111".to_string(),
                    token: "old-token".to_string(),
                },
            ]
        );

        let direct_cli_endpoints =
            discover_vscode_bridge_endpoints(temp.path(), RegistryMatch::RequireWorkspace);
        assert!(direct_cli_endpoints.is_empty());
    }

    #[test]
    fn bridge_registry_matches_shared_contract() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
            "USERPROFILE",
        ]);
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../docs/contracts/vscode-bridge-registry.json"
        ))
        .expect("registry contract parses");

        assert_eq!(
            BRIDGE_REGISTRY_MAX_AGE_MS,
            contract["ttlMs"].as_u64().unwrap()
        );
        for test_case in contract["registryDirs"].as_array().unwrap() {
            let env = test_case["env"].as_object().unwrap();
            let configured = env
                .get(VSCODE_BRIDGE_REGISTRY_DIR_ENV)
                .and_then(|value| value.as_str());
            let state_home = env.get("XDG_STATE_HOME").and_then(|value| value.as_str());
            let home = env.get("HOME").and_then(|value| value.as_str());
            let user_profile = env.get("USERPROFILE").and_then(|value| value.as_str());
            let actual =
                vscode_bridge_registry_dir_from_values(configured, state_home, home, user_profile);
            let expected = PathBuf::from(test_case["expected"].as_str().unwrap());
            assert_eq!(
                actual,
                expected,
                "registry dir contract case {:?}",
                test_case["name"].as_str()
            );
        }
        for test_case in contract["legacyRegistryDirs"].as_array().unwrap() {
            let env = test_case["env"].as_object().unwrap();
            let runtime_dir = env.get("XDG_RUNTIME_DIR").and_then(|value| value.as_str());
            let user = env
                .get("USER")
                .or_else(|| env.get("USERNAME"))
                .and_then(|value| value.as_str());
            let actual = legacy_vscode_bridge_registry_dirs_from_values(
                runtime_dir,
                user,
                Path::new(test_case["tmpDir"].as_str().unwrap()),
            );
            assert!(
                actual.contains(&PathBuf::from(test_case["expected"].as_str().unwrap())),
                "legacy registry dir contract case {:?}",
                test_case["name"].as_str()
            );
        }
        for test_case in contract["safeSegments"].as_array().unwrap() {
            assert_eq!(
                safe_registry_segment(test_case["input"].as_str().unwrap()),
                test_case["expected"].as_str().unwrap()
            );
        }
        for test_case in contract["expiry"]["cases"].as_array().unwrap() {
            assert_eq!(
                is_expired_registry_entry(
                    test_case["createdAtMs"].as_i64().unwrap() as u64,
                    contract["expiry"]["nowMs"].as_u64().unwrap()
                ),
                test_case["expired"].as_bool().unwrap()
            );
        }

        let temp = tempfile::tempdir().expect("tempdir");
        for item in contract["ordering"]["entries"].as_array().unwrap() {
            fs::write(
                temp.path().join(item["file"].as_str().unwrap()),
                item["entry"].to_string(),
            )
            .expect("registry entry");
        }
        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        env::remove_var(VSCODE_BRIDGE_URL_ENV);
        env::remove_var(VSCODE_BRIDGE_TOKEN_ENV);
        env::remove_var(VSCODE_BRIDGE_BYPASS_ENV);
        let allow_any = discover_vscode_bridge_registry_endpoints(
            Path::new(contract["ordering"]["cwd"].as_str().unwrap()),
            RegistryMatch::AllowAny,
            contract["ordering"]["nowMs"].as_u64().unwrap(),
        );
        let require_workspace = discover_vscode_bridge_registry_endpoints(
            Path::new(contract["ordering"]["cwd"].as_str().unwrap()),
            RegistryMatch::RequireWorkspace,
            contract["ordering"]["nowMs"].as_u64().unwrap(),
        );

        assert_eq!(
            endpoint_pairs(&allow_any),
            contract["ordering"]["expectedAllowAny"]
        );
        assert_eq!(
            endpoint_pairs(&require_workspace),
            contract["ordering"]["expectedRequireWorkspace"]
        );
    }

    #[test]
    fn bridge_registry_dir_value_helper_has_deterministic_last_resort() {
        let actual = vscode_bridge_registry_dir_from_values(None, None, None, None);

        assert_eq!(
            actual,
            PathBuf::from(".")
                .join(".local")
                .join("state")
                .join("dcmview")
                .join("vscode-bridges")
        );
    }

    #[test]
    fn bridge_discovery_uses_environment_then_registry_and_honors_bypass() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("registry.json"),
            serde_json::json!({
                "bridgeUrl": "http://127.0.0.1:1111",
                "token": "registry-token",
                "workspaceRoots": [temp.path()],
                "createdAtMs": now_unix_ms()
            })
            .to_string(),
        )
        .expect("registry");

        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        env::set_var(VSCODE_BRIDGE_URL_ENV, "http://127.0.0.1:2222");
        env::set_var(VSCODE_BRIDGE_TOKEN_ENV, "env-token");
        env::remove_var(VSCODE_BRIDGE_BYPASS_ENV);
        assert_eq!(
            discover_vscode_bridge_endpoints(temp.path(), RegistryMatch::RequireWorkspace),
            vec![
                BridgeEndpoint {
                    url: "http://127.0.0.1:2222".to_string(),
                    token: "env-token".to_string(),
                },
                BridgeEndpoint {
                    url: "http://127.0.0.1:1111".to_string(),
                    token: "registry-token".to_string(),
                },
            ]
        );

        env::set_var(VSCODE_BRIDGE_BYPASS_ENV, "1");
        assert!(discover_vscode_bridge_endpoints(temp.path(), RegistryMatch::AllowAny).is_empty());
    }

    #[test]
    fn expired_bridge_registry_entries_are_removed() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let expired_path = temp.path().join("expired.json");
        fs::write(
            &expired_path,
            serde_json::json!({
                "bridgeUrl": "http://127.0.0.1:1111",
                "token": "registry-token",
                "workspaceRoots": [],
                "createdAtMs": 1
            })
            .to_string(),
        )
        .expect("expired registry");

        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        env::remove_var(VSCODE_BRIDGE_URL_ENV);
        env::remove_var(VSCODE_BRIDGE_TOKEN_ENV);
        env::remove_var(VSCODE_BRIDGE_BYPASS_ENV);
        let endpoints = discover_vscode_bridge_registry_endpoints(
            temp.path(),
            RegistryMatch::AllowAny,
            BRIDGE_REGISTRY_MAX_AGE_MS + 2,
        );

        assert!(endpoints.is_empty());
        assert!(!expired_path.exists());
    }

    #[test]
    fn future_registry_versions_are_skipped_not_removed() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let future_path = temp.path().join("future.json");
        let malformed_v1_path = temp.path().join("malformed-v1.json");
        fs::write(
            &future_path,
            serde_json::json!({
                "version": 2,
                "createdAtMs": "future-format"
            })
            .to_string(),
        )
        .expect("future registry");
        fs::write(
            &malformed_v1_path,
            serde_json::json!({
                "version": 1,
                "bridgeUrl": "http://127.0.0.1:1111",
                "token": "token"
            })
            .to_string(),
        )
        .expect("malformed v1 registry");

        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        env::remove_var(VSCODE_BRIDGE_URL_ENV);
        env::remove_var(VSCODE_BRIDGE_TOKEN_ENV);
        env::remove_var(VSCODE_BRIDGE_BYPASS_ENV);
        let endpoints = discover_vscode_bridge_registry_endpoints(
            temp.path(),
            RegistryMatch::AllowAny,
            now_unix_ms(),
        );

        assert!(endpoints.is_empty());
        assert!(future_path.exists());
        assert!(!malformed_v1_path.exists());
    }

    #[test]
    fn bridge_launch_cleanup_uses_typed_error_signal() {
        let registry_endpoint = BridgeEndpoint {
            url: "http://127.0.0.1:1111".to_string(),
            token: "registry-token".to_string(),
        };
        let env_endpoint = BridgeEndpoint {
            url: "http://127.0.0.1:2222".to_string(),
            token: "env-token".to_string(),
        };
        let registry_endpoints = vec![registry_endpoint.clone()];

        assert!(should_remove_registry_entry_after_launch_error(
            &BridgeLaunchError::Connect("connection refused".to_string()),
            &registry_endpoint,
            &registry_endpoints,
        ));
        assert!(!should_remove_registry_entry_after_launch_error(
            &BridgeLaunchError::Connect("connection refused".to_string()),
            &env_endpoint,
            &registry_endpoints,
        ));
        assert!(!should_remove_registry_entry_after_launch_error(
            &BridgeLaunchError::Http {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                message: "failed".to_string(),
            },
            &registry_endpoint,
            &registry_endpoints,
        ));
    }

    #[cfg(unix)]
    #[test]
    fn untrusted_registry_directory_ownership_is_rejected() {
        assert!(registry_ownership_is_trusted(1000, 0o700, 1000));
        assert!(!registry_ownership_is_trusted(1001, 0o700, 1000));
        assert!(!registry_ownership_is_trusted(1000, 0o722, 1000));
    }

    #[test]
    fn removing_bridge_registry_endpoint_deletes_matching_entries_only() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _env_guard = EnvGuard::capture(&[
            VSCODE_BRIDGE_REGISTRY_DIR_ENV,
            VSCODE_BRIDGE_URL_ENV,
            VSCODE_BRIDGE_TOKEN_ENV,
            VSCODE_BRIDGE_BYPASS_ENV,
        ]);
        let temp = tempfile::tempdir().expect("tempdir");
        let stale_path = temp.path().join("stale.json");
        let live_path = temp.path().join("live.json");
        fs::write(
            &stale_path,
            serde_json::json!({
                "bridgeUrl": "http://127.0.0.1:1111",
                "token": "stale-token",
                "workspaceRoots": [],
                "createdAtMs": now_unix_ms()
            })
            .to_string(),
        )
        .expect("stale registry");
        fs::write(
            &live_path,
            serde_json::json!({
                "bridgeUrl": "http://127.0.0.1:2222",
                "token": "live-token",
                "workspaceRoots": [],
                "createdAtMs": now_unix_ms()
            })
            .to_string(),
        )
        .expect("live registry");

        env::set_var(VSCODE_BRIDGE_REGISTRY_DIR_ENV, temp.path());
        remove_vscode_bridge_registry_endpoint(&BridgeEndpoint {
            url: "http://127.0.0.1:1111".to_string(),
            token: "stale-token".to_string(),
        });

        assert!(!stale_path.exists());
        assert!(live_path.exists());
    }

    struct EnvGuard {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn capture(keys: &[&'static str]) -> Self {
            Self {
                values: keys.iter().map(|key| (*key, env::var_os(key))).collect(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.values.drain(..) {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }

    fn endpoint_pairs(endpoints: &[BridgeEndpoint]) -> serde_json::Value {
        serde_json::Value::Array(
            endpoints
                .iter()
                .map(|endpoint| serde_json::json!([endpoint.url, endpoint.token]))
                .collect(),
        )
    }
}
