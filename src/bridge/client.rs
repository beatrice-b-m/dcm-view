use super::protocol::{BridgeLaunchRequest, BridgeLaunchResponse, BridgeWaitResponse};
use super::registry::{
    bridge_debug, discover_vscode_bridge_endpoints, discover_vscode_bridge_registry_endpoints,
    remove_vscode_bridge_registry_endpoint, BridgeEndpoint, RegistryMatch,
    VSCODE_BRIDGE_BYPASS_ENV,
};
use anyhow::{Context, Result};
use dcmview::server::now_unix_ms;
use std::env;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use tokio::process::Command;

const BRIDGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

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

#[derive(Debug, Clone)]
struct ClientContext {
    cwd: PathBuf,
    current_executable: std::result::Result<PathBuf, String>,
    registry_endpoints: Vec<BridgeEndpoint>,
    request_timeout: Duration,
}

impl ClientContext {
    fn capture(cwd: PathBuf, registry_endpoints: Vec<BridgeEndpoint>) -> Self {
        Self {
            cwd,
            current_executable: env::current_exe().map_err(|error| error.to_string()),
            registry_endpoints,
            request_timeout: BRIDGE_REQUEST_TIMEOUT,
        }
    }

    fn launch_request(&self, program: &str, args: &[String]) -> BridgeLaunchRequest {
        BridgeLaunchRequest {
            program: program.to_string(),
            args: args.to_vec(),
            cwd: self.cwd.display().to_string(),
            wait: false,
            binary_path: self
                .current_executable
                .as_ref()
                .ok()
                .map(|path| path.display().to_string()),
        }
    }

    fn fallback_request(&self, args: &[String]) -> Result<ProcessRequest> {
        let executable = self.current_executable.as_ref().map_err(|error| {
            anyhow::Error::msg(error.clone()).context("failed to resolve current executable")
        })?;
        Ok(ProcessRequest::local_fallback(executable.clone(), args))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessRequest {
    executable: PathBuf,
    args: Vec<String>,
    environment: Vec<(String, String)>,
}

impl ProcessRequest {
    fn local_fallback(executable: PathBuf, args: &[String]) -> Self {
        Self {
            executable,
            args: args.to_vec(),
            environment: vec![(VSCODE_BRIDGE_BYPASS_ENV.to_string(), "1".to_string())],
        }
    }
}

type ProcessFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Option<i32>>> + Send + 'a>>;

trait ProcessRunner: Send + Sync {
    fn run<'a>(&'a self, request: &'a ProcessRequest) -> ProcessFuture<'a>;
}

struct TokioProcessRunner;

impl ProcessRunner for TokioProcessRunner {
    fn run<'a>(&'a self, request: &'a ProcessRequest) -> ProcessFuture<'a> {
        Box::pin(async move {
            let mut command = Command::new(&request.executable);
            command.args(&request.args);
            for (key, value) in &request.environment {
                command.env(key, value);
            }
            let status = command.status().await?;
            Ok(status.code())
        })
    }
}

trait RegistryEndpointRemover: Send + Sync {
    fn remove(&self, endpoint: &BridgeEndpoint);
}

struct ProductionRegistryEndpointRemover;

impl RegistryEndpointRemover for ProductionRegistryEndpointRemover {
    fn remove(&self, endpoint: &BridgeEndpoint) {
        remove_vscode_bridge_registry_endpoint(endpoint);
    }
}

pub(crate) async fn run_vscode_bridge_client(values: Vec<String>) -> Result<i32> {
    let Some((program, args)) = values.split_first() else {
        return Ok(1);
    };

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let bridge_endpoints = discover_vscode_bridge_endpoints(&cwd, RegistryMatch::AllowAny);
    let registry_endpoints = if bridge_endpoints.is_empty() {
        Vec::new()
    } else {
        discover_vscode_bridge_registry_endpoints(&cwd, RegistryMatch::AllowAny, now_unix_ms())
    };
    let context = ClientContext::capture(cwd, registry_endpoints);
    let process_runner = TokioProcessRunner;
    let registry_remover = ProductionRegistryEndpointRemover;
    let http_client = if bridge_endpoints.is_empty() {
        None
    } else {
        Some(build_http_client()?)
    };

    run_vscode_bridge_client_with_dependencies(
        program,
        args,
        &bridge_endpoints,
        &context,
        http_client.as_ref(),
        &process_runner,
        &registry_remover,
    )
    .await
}

pub(crate) async fn run_vscode_bridge_launch(
    program: &str,
    args: &[String],
    bridge_endpoints: &[BridgeEndpoint],
) -> Result<i32> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let registry_endpoints =
        discover_vscode_bridge_registry_endpoints(&cwd, RegistryMatch::AllowAny, now_unix_ms());
    let context = ClientContext::capture(cwd, registry_endpoints);
    let client = build_http_client()?;
    let registry_remover = ProductionRegistryEndpointRemover;

    run_vscode_bridge_launch_with_dependencies(
        program,
        args,
        bridge_endpoints,
        &context,
        &client,
        &registry_remover,
    )
    .await
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .build()
        .context("failed to create VS Code bridge HTTP client")
}

async fn run_vscode_bridge_client_with_dependencies(
    program: &str,
    args: &[String],
    bridge_endpoints: &[BridgeEndpoint],
    context: &ClientContext,
    client: Option<&reqwest::Client>,
    process_runner: &dyn ProcessRunner,
    registry_remover: &dyn RegistryEndpointRemover,
) -> Result<i32> {
    if bridge_endpoints.is_empty() {
        return fallback_to_local_viewer(context, args, process_runner).await;
    }
    let client =
        client.ok_or_else(|| anyhow::anyhow!("VS Code bridge HTTP client is unavailable"))?;

    match run_vscode_bridge_launch_with_dependencies(
        program,
        args,
        bridge_endpoints,
        context,
        client,
        registry_remover,
    )
    .await
    {
        Ok(exit_code) => Ok(exit_code),
        Err(error) => {
            eprintln!(
                "dcmview: VS Code bridge unavailable ({error}); falling back to local viewer"
            );
            fallback_to_local_viewer(context, args, process_runner).await
        }
    }
}

async fn run_vscode_bridge_launch_with_dependencies(
    program: &str,
    args: &[String],
    bridge_endpoints: &[BridgeEndpoint],
    context: &ClientContext,
    client: &reqwest::Client,
    registry_remover: &dyn RegistryEndpointRemover,
) -> Result<i32> {
    let launch = context.launch_request(program, args);
    let mut last_error = None;
    for endpoint in bridge_endpoints {
        match launch_vscode_session(client, endpoint, &launch, context.request_timeout).await {
            Ok(launch_response) => {
                return match wait_for_launched_vscode_session(
                    client,
                    endpoint,
                    launch_response,
                    context.request_timeout,
                )
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
                    &context.registry_endpoints,
                ) {
                    registry_remover.remove(endpoint);
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
    request_timeout: Duration,
) -> std::result::Result<BridgeLaunchResponse, BridgeLaunchError> {
    let launch_url = format!("{}/launch", endpoint.url.trim_end_matches('/'));
    let response = match client
        .post(launch_url)
        .bearer_auth(&endpoint.token)
        .json(launch)
        .timeout(request_timeout)
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
    request_timeout: Duration,
) -> Result<i32> {
    wait_for_launched_vscode_session_with_interrupt(
        client,
        endpoint,
        launch_response,
        request_timeout,
        wait_for_bridge_interrupt(),
    )
    .await
}

async fn wait_for_launched_vscode_session_with_interrupt<F>(
    client: &reqwest::Client,
    endpoint: &BridgeEndpoint,
    launch_response: BridgeLaunchResponse,
    request_timeout: Duration,
    interrupt: F,
) -> Result<i32>
where
    F: Future<Output = Result<()>>,
{
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

    tokio::select! {
        wait_result = wait_for_vscode_session(client, &wait_url, &endpoint.token) => wait_result,
        signal_result = interrupt => {
            signal_result?;
            let _ = client
                .post(stop_url)
                .bearer_auth(&endpoint.token)
                .timeout(request_timeout)
                .send()
                .await;
            Ok(130)
        }
    }
}

async fn wait_for_bridge_interrupt() -> Result<()> {
    #[cfg(not(windows))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("failed to listen for Ctrl+C")
    }

    #[cfg(windows)]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        let ctrl_break = async {
            let mut signal =
                tokio::signal::windows::ctrl_break().context("failed to listen for Ctrl+Break")?;
            signal.recv().await;
            Ok::<(), anyhow::Error>(())
        };

        tokio::select! {
            signal_result = ctrl_c => {
                signal_result.context("failed to listen for Ctrl+C")
            },
            signal_result = ctrl_break => signal_result,
        }
    }
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

async fn fallback_to_local_viewer(
    context: &ClientContext,
    args: &[String],
    process_runner: &dyn ProcessRunner,
) -> Result<i32> {
    let request = context.fallback_request(args)?;
    let exit_code = process_runner
        .run(&request)
        .await
        .context("failed to run local dcmview fallback")?;
    Ok(exit_code.unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;
    use tokio::task::JoinHandle;

    #[derive(Debug, Clone, Copy)]
    enum ProcessOutcome {
        Exit(Option<i32>),
        Error(io::ErrorKind),
    }

    struct RecordingProcessRunner {
        requests: Mutex<Vec<ProcessRequest>>,
        outcome: ProcessOutcome,
    }

    impl RecordingProcessRunner {
        fn exiting(exit_code: Option<i32>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                outcome: ProcessOutcome::Exit(exit_code),
            }
        }

        fn failing(kind: io::ErrorKind) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                outcome: ProcessOutcome::Error(kind),
            }
        }

        fn requests(&self) -> Vec<ProcessRequest> {
            self.requests.lock().expect("process requests").clone()
        }
    }

    impl ProcessRunner for RecordingProcessRunner {
        fn run<'a>(&'a self, request: &'a ProcessRequest) -> ProcessFuture<'a> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .expect("process requests")
                    .push(request.clone());
                match self.outcome {
                    ProcessOutcome::Exit(exit_code) => Ok(exit_code),
                    ProcessOutcome::Error(kind) => {
                        Err(io::Error::new(kind, "injected process failure"))
                    }
                }
            })
        }
    }

    #[derive(Default)]
    struct RecordingRegistryRemover {
        endpoints: Mutex<Vec<BridgeEndpoint>>,
    }

    impl RecordingRegistryRemover {
        fn endpoints(&self) -> Vec<BridgeEndpoint> {
            self.endpoints.lock().expect("removed endpoints").clone()
        }
    }

    impl RegistryEndpointRemover for RecordingRegistryRemover {
        fn remove(&self, endpoint: &BridgeEndpoint) {
            self.endpoints
                .lock()
                .expect("removed endpoints")
                .push(endpoint.clone());
        }
    }

    fn endpoint(url: impl Into<String>) -> BridgeEndpoint {
        BridgeEndpoint {
            url: url.into(),
            token: "bridge-token".to_string(),
        }
    }

    fn test_context(registry_endpoints: Vec<BridgeEndpoint>) -> ClientContext {
        ClientContext {
            cwd: PathBuf::from("/workspace"),
            current_executable: Ok(PathBuf::from("/opt/dcmview/bin/dcmview")),
            registry_endpoints,
            request_timeout: Duration::from_millis(100),
        }
    }

    #[tokio::test]
    async fn fallback_request_sets_bypass_and_preserves_exit_code() {
        let runner = RecordingProcessRunner::exiting(Some(7));
        let context = test_context(Vec::new());
        let args = vec!["scan.dcm".to_string(), "--no-browser".to_string()];

        let exit_code = fallback_to_local_viewer(&context, &args, &runner)
            .await
            .expect("fallback succeeds");

        assert_eq!(exit_code, 7);
        assert_eq!(
            runner.requests(),
            vec![ProcessRequest {
                executable: PathBuf::from("/opt/dcmview/bin/dcmview"),
                args,
                environment: vec![("DCMVIEW_VSCODE_BYPASS".to_string(), "1".to_string())],
            }]
        );
    }

    #[tokio::test]
    async fn fallback_maps_signal_exit_to_one_and_preserves_launch_failure_context() {
        let context = test_context(Vec::new());
        let signaled_runner = RecordingProcessRunner::exiting(None);
        assert_eq!(
            fallback_to_local_viewer(&context, &[], &signaled_runner)
                .await
                .expect("signal exit maps"),
            1
        );

        let failing_runner = RecordingProcessRunner::failing(io::ErrorKind::PermissionDenied);
        let error = fallback_to_local_viewer(&context, &[], &failing_runner)
            .await
            .expect_err("process launch fails");
        assert!(error
            .to_string()
            .contains("failed to run local dcmview fallback"));
    }

    #[tokio::test]
    async fn bridge_client_launch_failure_falls_back_without_recursing() {
        let dead_url = unused_loopback_url().await;
        let direct_endpoint = endpoint(dead_url);
        let context = test_context(Vec::new());
        let client = reqwest::Client::new();
        let runner = RecordingProcessRunner::exiting(Some(23));
        let remover = RecordingRegistryRemover::default();
        let args = vec!["scan.dcm".to_string()];

        let exit_code = run_vscode_bridge_client_with_dependencies(
            "dcmview",
            &args,
            std::slice::from_ref(&direct_endpoint),
            &context,
            Some(&client),
            &runner,
            &remover,
        )
        .await
        .expect("fallback succeeds");

        assert_eq!(exit_code, 23);
        assert_eq!(remover.endpoints(), Vec::<BridgeEndpoint>::new());
        let requests = runner.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].environment,
            vec![(VSCODE_BRIDGE_BYPASS_ENV.to_string(), "1".to_string())]
        );
    }

    #[derive(Clone, Default)]
    struct MockBridgeState {
        events: Arc<Mutex<Vec<String>>>,
        launch_requests: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    async fn authenticated_launch(
        State(state): State<MockBridgeState>,
        headers: HeaderMap,
        Json(request): Json<BridgeLaunchRequest>,
    ) -> Response {
        if bearer_token(&headers) != Some("bridge-token") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        state
            .events
            .lock()
            .expect("mock events")
            .push(format!("launch:{}:{}", request.program, request.cwd));
        state
            .launch_requests
            .lock()
            .expect("mock launch requests")
            .push(serde_json::to_value(request).expect("serialize mock launch request"));
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "sessionId": "session-1",
                "url": "http://127.0.0.1:51234"
            })),
        )
            .into_response()
    }

    async fn authenticated_wait(
        Path(session_id): Path<String>,
        State(state): State<MockBridgeState>,
        headers: HeaderMap,
    ) -> Response {
        if bearer_token(&headers) != Some("bridge-token") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        state
            .events
            .lock()
            .expect("mock events")
            .push(format!("wait:{session_id}"));
        (StatusCode::OK, Json(serde_json::json!({ "exitCode": 7 }))).into_response()
    }

    fn bearer_token(headers: &HeaderMap) -> Option<&str> {
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
    }

    #[tokio::test]
    async fn authenticated_launch_and_wait_round_trip_against_axum_bridge() {
        let state = MockBridgeState::default();
        let server = MockServer::spawn(
            Router::new()
                .route("/launch", post(authenticated_launch))
                .route("/sessions/{session_id}/wait", get(authenticated_wait))
                .with_state(state.clone()),
        )
        .await;
        let bridge_endpoint = endpoint(format!("{}/", server.url()));
        let context = test_context(Vec::new());
        let remover = RecordingRegistryRemover::default();

        let exit_code = run_vscode_bridge_launch_with_dependencies(
            "dcmview",
            &["scan.dcm".to_string()],
            std::slice::from_ref(&bridge_endpoint),
            &context,
            &reqwest::Client::new(),
            &remover,
        )
        .await
        .expect("bridge launch succeeds");

        assert_eq!(exit_code, 7);
        assert_eq!(
            *state.events.lock().expect("mock events"),
            vec![
                "launch:dcmview:/workspace".to_string(),
                "wait:session-1".to_string()
            ]
        );
        assert_eq!(
            *state.launch_requests.lock().expect("mock launch requests"),
            vec![serde_json::json!({
                "program": "dcmview",
                "args": ["scan.dcm"],
                "cwd": "/workspace",
                "wait": false,
                "binaryPath": "/opt/dcmview/bin/dcmview"
            })]
        );
        assert!(remover.endpoints().is_empty());
    }

    #[derive(Clone, Default)]
    struct InterruptBridgeState {
        wait_started: Arc<Notify>,
        stop_events: Arc<Mutex<Vec<String>>>,
    }

    async fn pending_wait(
        Path(session_id): Path<String>,
        State(state): State<InterruptBridgeState>,
        headers: HeaderMap,
    ) -> Response {
        if bearer_token(&headers) != Some("bridge-token") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        assert_eq!(session_id, "session-1");
        state.wait_started.notify_one();
        std::future::pending::<Response>().await
    }

    async fn record_stop(
        Path(session_id): Path<String>,
        State(state): State<InterruptBridgeState>,
        headers: HeaderMap,
    ) -> Response {
        let token = bearer_token(&headers).unwrap_or("missing");
        state
            .stop_events
            .lock()
            .expect("stop events")
            .push(format!("{session_id}:{token}"));
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }

    #[tokio::test]
    async fn interrupt_posts_authenticated_stop_and_ignores_stop_failure() {
        let state = InterruptBridgeState::default();
        let server = MockServer::spawn(
            Router::new()
                .route("/sessions/{session_id}/wait", get(pending_wait))
                .route("/sessions/{session_id}/stop", post(record_stop))
                .with_state(state.clone()),
        )
        .await;
        let wait_started = state.wait_started.clone();
        let interrupt = async move {
            wait_started.notified().await;
            Ok(())
        };

        let exit_code = tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_launched_vscode_session_with_interrupt(
                &reqwest::Client::new(),
                &endpoint(server.url()),
                BridgeLaunchResponse {
                    session_id: "session-1".to_string(),
                    url: "http://127.0.0.1:51234".to_string(),
                },
                Duration::from_millis(100),
                interrupt,
            ),
        )
        .await
        .expect("interrupt flow completes")
        .expect("interrupt flow succeeds");

        assert_eq!(exit_code, 130);
        assert_eq!(
            *state.stop_events.lock().expect("stop events"),
            vec!["session-1:bridge-token".to_string()]
        );
    }

    async fn http_error() -> Response {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "bridge unavailable" })),
        )
            .into_response()
    }

    async fn invalid_launch_response() -> Response {
        (StatusCode::OK, Json(serde_json::json!({ "sessionId": 42 }))).into_response()
    }

    #[tokio::test]
    async fn launch_errors_classify_http_decode_connect_and_request_failures() {
        let client = reqwest::Client::new();
        let launch = test_context(Vec::new()).launch_request("dcmview", &[]);

        let http_server = MockServer::spawn(Router::new().route("/launch", post(http_error))).await;
        let http = launch_vscode_session(
            &client,
            &endpoint(http_server.url()),
            &launch,
            Duration::from_secs(1),
        )
        .await
        .expect_err("HTTP status is classified");
        assert!(matches!(
            http,
            BridgeLaunchError::Http {
                status: StatusCode::SERVICE_UNAVAILABLE,
                ref message,
            } if message == "bridge unavailable"
        ));

        let decode_server =
            MockServer::spawn(Router::new().route("/launch", post(invalid_launch_response))).await;
        let decode = launch_vscode_session(
            &client,
            &endpoint(decode_server.url()),
            &launch,
            Duration::from_secs(1),
        )
        .await
        .expect_err("invalid JSON shape is classified");
        assert!(matches!(decode, BridgeLaunchError::Decode(_)));

        let connect = launch_vscode_session(
            &client,
            &endpoint(unused_loopback_url().await),
            &launch,
            Duration::from_millis(100),
        )
        .await
        .expect_err("connection failure is classified");
        assert!(matches!(connect, BridgeLaunchError::Connect(_)));

        let request = launch_vscode_session(
            &client,
            &endpoint("not a URL"),
            &launch,
            Duration::from_secs(1),
        )
        .await
        .expect_err("request construction is classified");
        assert!(matches!(request, BridgeLaunchError::Request(_)));
    }

    async fn delayed_launch() -> Response {
        tokio::time::sleep(Duration::from_millis(250)).await;
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "sessionId": "late",
                "url": "http://127.0.0.1:51234"
            })),
        )
            .into_response()
    }

    #[tokio::test]
    async fn timeout_removes_only_matching_registry_endpoint() {
        let server = MockServer::spawn(Router::new().route("/launch", post(delayed_launch))).await;
        let registry_endpoint = endpoint(server.url());
        let mut context = test_context(vec![registry_endpoint.clone()]);
        context.request_timeout = Duration::from_millis(20);
        let remover = RecordingRegistryRemover::default();

        let error = run_vscode_bridge_launch_with_dependencies(
            "dcmview",
            &[],
            std::slice::from_ref(&registry_endpoint),
            &context,
            &reqwest::Client::new(),
            &remover,
        )
        .await
        .expect_err("timeout fails launch");

        assert!(error
            .to_string()
            .contains("failed to contact VS Code bridge"));
        assert_eq!(remover.endpoints(), vec![registry_endpoint]);
    }

    #[test]
    fn cleanup_policy_excludes_non_connect_errors_and_direct_endpoints() {
        let registry_endpoint = endpoint("http://127.0.0.1:1111");
        let direct_endpoint = endpoint("http://127.0.0.1:2222");
        let registry_endpoints = vec![registry_endpoint.clone()];

        assert!(should_remove_registry_entry_after_launch_error(
            &BridgeLaunchError::Connect("connection refused".to_string()),
            &registry_endpoint,
            &registry_endpoints,
        ));
        assert!(!should_remove_registry_entry_after_launch_error(
            &BridgeLaunchError::Connect("request timed out".to_string()),
            &direct_endpoint,
            &registry_endpoints,
        ));
        for error in [
            BridgeLaunchError::Http {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                message: "failed".to_string(),
            },
            BridgeLaunchError::Decode("invalid response".to_string()),
            BridgeLaunchError::Request("request failed".to_string()),
        ] {
            assert!(!should_remove_registry_entry_after_launch_error(
                &error,
                &registry_endpoint,
                &registry_endpoints,
            ));
        }
    }

    struct MockServer {
        base_url: String,
        task: JoinHandle<()>,
    }

    impl MockServer {
        async fn spawn(router: Router) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock bridge");
            let address = listener.local_addr().expect("mock bridge address");
            let task = tokio::spawn(async move {
                axum::serve(listener, router)
                    .await
                    .expect("serve mock bridge");
            });
            Self {
                base_url: format!("http://{address}"),
                task,
            }
        }

        fn url(&self) -> String {
            self.base_url.clone()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn unused_loopback_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unused endpoint");
        let address = listener.local_addr().expect("unused endpoint address");
        drop(listener);
        format!("http://{address}")
    }
}
