use crate::annotations::AnnotationStore;
use crate::api::contracts::{
    self as api_contracts, ApiEndpointContract, ApiEndpointSpec, ApiMethod, ApiOperation,
    EmbedRoiAnnotations, ErrorResponse, FileSummary, FilesResponse, FrameInfo, FrameQuery,
    HealthResponse, TagNode, TagValue, API_ENDPOINTS, API_PREFIX, CACHE_HEADER, CACHE_HIT,
    CACHE_MISS, EXPORT_CONTENT_DISPOSITION_HEADER, EXPORT_CONTENT_DISPOSITION_VALUE,
    RAW_FRAME_HEADER_BITS_ALLOCATED, RAW_FRAME_HEADER_COLUMNS, RAW_FRAME_HEADER_DEFAULT_WC,
    RAW_FRAME_HEADER_DEFAULT_WW, RAW_FRAME_HEADER_PHOTOMETRIC_INTERPRETATION,
    RAW_FRAME_HEADER_PIXEL_REPRESENTATION, RAW_FRAME_HEADER_RESCALE_INTERCEPT,
    RAW_FRAME_HEADER_RESCALE_SLOPE, RAW_FRAME_HEADER_ROWS, RAW_FRAME_HEADER_SAMPLES_PER_PIXEL,
};
use crate::pixels::{self, FrameCache, FrameRequest, RawFrameCache, RawFrameRequest};
use crate::tunnel::{self, TunnelHandle};
use crate::types::{FileEntry, TunnelInfo};
use anyhow::{Context, Result};
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::header::HasLength;
use dicom_dictionary_std::StandardDataDictionary;
use dicom_object::{open_file, InMemDicomObject};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::collections::HashMap;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{futures::Notified, Notify};
#[cfg(feature = "debug-api")]
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub registry: FileRegistry,
    pub pixel_cache: Arc<Mutex<FrameCache>>,
    pub raw_cache: Arc<Mutex<RawFrameCache>>,
    pub tag_cache: Arc<Mutex<HashMap<usize, Vec<TagNode>>>>,
    pub annotations: AnnotationStore,
    pub tunnel_info: Option<Arc<TunnelInfo>>,
    pub tunnel_handle: Option<Arc<TunnelHandle>>,
    pub server_start_ms: u64,
    pub last_request: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct FileRegistry {
    inner: Arc<RwLock<FileRegistryInner>>,
    scanned: Arc<AtomicUsize>,
    skipped: Arc<AtomicUsize>,
    filtered: Arc<AtomicUsize>,
    scan_complete: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct FileRegistryInner {
    files: Vec<FileEntry>,
    summaries: Vec<FileSummary>,
}

#[derive(Debug, Clone, Copy)]
pub struct RegistryStatus {
    pub file_count: usize,
    pub scanned: usize,
    pub skipped: usize,
    pub filtered: usize,
    pub scan_complete: bool,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FileRegistryInner::default())),
            scanned: Arc::new(AtomicUsize::new(0)),
            skipped: Arc::new(AtomicUsize::new(0)),
            filtered: Arc::new(AtomicUsize::new(0)),
            scan_complete: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn from_files(files: Vec<FileEntry>) -> Self {
        let registry = Self::new();
        for file in files {
            registry.insert(file);
            registry.record_scanned();
        }
        registry.mark_scan_complete();
        registry
    }

    pub fn insert(&self, mut file: FileEntry) -> usize {
        let mut inner = self.inner.write().expect("file registry lock poisoned");
        let index = inner.files.len();
        file.index = index;
        let summary = FileSummary::from(&file);
        inner.files.push(file);
        inner.summaries.push(summary);
        drop(inner);
        self.notify.notify_waiters();
        index
    }

    pub fn record_scanned(&self) {
        self.scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_filtered(&self) {
        self.filtered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn mark_scan_complete(&self) {
        self.scan_complete.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub fn changed(&self) -> Notified<'_> {
        self.notify.notified()
    }

    pub fn get(&self, index: usize) -> Option<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .get(index)
            .cloned()
    }

    pub fn files_snapshot(&self) -> Vec<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .clone()
    }

    pub fn summaries_snapshot(&self) -> Vec<FileSummary> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .summaries
            .clone()
    }

    pub fn status(&self) -> RegistryStatus {
        let file_count = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .len();
        RegistryStatus {
            file_count,
            scanned: self.scanned.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            filtered: self.filtered.load(Ordering::Relaxed),
            scan_complete: self.scan_complete.load(Ordering::Relaxed),
        }
    }
}

impl Default for FileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TunnelConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub timeout_seconds: Option<u64>,
    pub open_browser: bool,
    pub startup_json: bool,
    pub tunnel: Option<TunnelConfig>,
    pub shutdown: Option<Arc<Notify>>,
}

#[derive(Debug, Serialize)]
struct StartupEvent<'a> {
    r#type: &'a str,
    url: &'a str,
    host: &'a str,
    port: u16,
}

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

pub async fn run(config: ServerConfig, mut state: AppState) -> Result<()> {
    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind to {bind_addr}"))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read local bind address")?;
    let server_url = format!("http://{}:{}", local_addr.ip(), local_addr.port());

    if config.startup_json {
        println!(
            "{}",
            startup_event_json(&server_url, &config.host, local_addr.port())
                .context("failed to serialize startup event")?
        );
    }
    println!("dcmview: server running at {server_url}");
    if is_non_loopback_bind(local_addr.ip()) {
        eprintln!(
			"dcmview: warning — server bound to non-loopback address {}; endpoints are unauthenticated and may expose sensitive DICOM data",
			local_addr.ip()
		);
        eprintln!("dcmview: warning — prefer --host 127.0.0.1 (or ::1) and use --tunnel for remote access");
    }

    if let Some(tunnel_cfg) = config.tunnel {
        let runtime =
            tunnel::start_tunnel(local_addr.port(), tunnel_cfg.host.clone(), tunnel_cfg.port)?;
        if let Some(warning) = runtime.warning.as_deref() {
            eprintln!("{warning}");
            eprintln!("dcmview: to forward manually, run on your local machine:");
            eprintln!(
                "dcmview:   ssh -L {0}:localhost:{0} {1}",
                runtime.info.tunnel_port, runtime.info.tunnel_host
            );
        } else {
            println!(
                "dcmview: SSH tunnel active — access at http://localhost:{} on your local machine",
                runtime.info.tunnel_port
            );
        }
        state.tunnel_info = Some(Arc::new(runtime.info));
        state.tunnel_handle = runtime.handle.map(Arc::new);
    } else {
        println!(
			"dcmview: (on a remote server? run on your local machine: ssh -L {0}:localhost:{0} user@host)",
			local_addr.port()
		);
    }

    if config.open_browser {
        spawn_browser_opener(server_url.clone(), state.registry.clone());
    }

    println!("dcmview: press Ctrl+C to stop");

    if let Some(timeout) = config.timeout_seconds {
        spawn_idle_timeout_watcher(
            timeout,
            state.last_request.clone(),
            state.registry.clone(),
            state.tunnel_handle.clone(),
        );
    }

    let tunnel_handle = state.tunnel_handle.clone();
    let shutdown_notify = config.shutdown.clone();
    let app = router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(tunnel_handle, shutdown_notify))
        .await
        .context("server failed")
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn is_non_loopback_bind(ip: std::net::IpAddr) -> bool {
    !ip.is_loopback()
}

pub fn router(state: AppState) -> Router {
    let api = API_ENDPOINTS
        .iter()
        .fold(Router::new(), register_api_endpoint)
        .fallback(api_not_found_handler)
        .method_not_allowed_fallback(api_method_not_allowed_handler);

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/assets/{*path}", get(asset_handler))
        .nest(API_PREFIX, api)
        .with_state(state);

    #[cfg(feature = "debug-api")]
    let router = router.layer(CorsLayer::permissive());

    router
}

trait HandlerResponseContract {
    type Output;
}

macro_rules! json_handler_response {
    ($($response:ty),+ $(,)?) => {
        $(
            impl HandlerResponseContract for $response {
                type Output = Json<$response>;
            }
        )+
    };
}

json_handler_response!(
    HealthResponse,
    FilesResponse,
    FrameInfo,
    Vec<TagNode>,
    EmbedRoiAnnotations,
);

impl HandlerResponseContract for api_contracts::BlobBody {
    type Output = Response;
}

impl HandlerResponseContract for api_contracts::ArrayBufferBody {
    type Output = Response;
}

trait HandlerErrorContract {
    type Output;
}

impl HandlerErrorContract for ErrorResponse {
    type Output = ApiError;
}

macro_rules! handler_response_type {
    ($spec:ty) => {
        <<$spec as ApiEndpointSpec>::Response as HandlerResponseContract>::Output
    };
}

macro_rules! handler_result_type {
    ($spec:ty) => {
        std::result::Result<
            handler_response_type!($spec),
            <<$spec as ApiEndpointSpec>::Error as HandlerErrorContract>::Output,
        >
    };
}

fn require_endpoint_spec<Spec>(endpoint: &ApiEndpointContract)
where
    Spec: ApiEndpointSpec,
    Spec::Response: HandlerResponseContract,
    Spec::Error: HandlerErrorContract,
{
    assert_eq!(
        *endpoint,
        Spec::CONTRACT,
        "registered endpoint does not match its typed handler specification"
    );
}

fn require_no_query<Spec>()
where
    Spec: ApiEndpointSpec<Query = api_contracts::NoQuery>,
{
}

fn require_no_request<Spec>()
where
    Spec: ApiEndpointSpec<Request = api_contracts::NoRequest>,
{
}

fn register_api_endpoint(
    router: Router<AppState>,
    endpoint: &ApiEndpointContract,
) -> Router<AppState> {
    let require_method = |expected| {
        assert_eq!(
            endpoint.method, expected,
            "API endpoint {} has a method that does not match its handler",
            endpoint.id
        );
    };

    match endpoint.operation {
        ApiOperation::Health => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::Health>(endpoint);
            require_no_query::<api_contracts::Health>();
            require_no_request::<api_contracts::Health>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_response_type!(api_contracts::Health) =
                        health_handler(state).await;
                    response
                }),
            )
        }
        ApiOperation::Files => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::Files>(endpoint);
            require_no_query::<api_contracts::Files>();
            require_no_request::<api_contracts::Files>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_response_type!(api_contracts::Files) =
                        files_handler(state).await;
                    response
                }),
            )
        }
        ApiOperation::FileInfo => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileInfo>(endpoint);
            require_no_query::<api_contracts::FileInfo>();
            require_no_request::<api_contracts::FileInfo>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: std::result::Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileInfo) =
                            info_handler(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileFrame => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileFrame>(endpoint);
            require_no_request::<api_contracts::FileFrame>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: std::result::Result<Path<(usize, u32)>, PathRejection>,
                     query: std::result::Result<
                        Query<<api_contracts::FileFrame as ApiEndpointSpec>::Query>,
                        QueryRejection,
                    >| async move {
                        let response: handler_result_type!(api_contracts::FileFrame) =
                            frame_handler(state, path, query).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileRawFrame => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileRawFrame>(endpoint);
            require_no_query::<api_contracts::FileRawFrame>();
            require_no_request::<api_contracts::FileRawFrame>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: std::result::Result<Path<(usize, u32)>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileRawFrame) =
                            raw_frame_handler(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileTags => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileTags>(endpoint);
            require_no_query::<api_contracts::FileTags>();
            require_no_request::<api_contracts::FileTags>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: std::result::Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileTags) =
                            tags_handler(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileAnnotationsGet => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileAnnotationsGet>(endpoint);
            require_no_query::<api_contracts::FileAnnotationsGet>();
            require_no_request::<api_contracts::FileAnnotationsGet>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: std::result::Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileAnnotationsGet) =
                            annotations_handler(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileAnnotationsUpdate => {
            require_method(ApiMethod::Put);
            require_endpoint_spec::<api_contracts::FileAnnotationsUpdate>(endpoint);
            require_no_query::<api_contracts::FileAnnotationsUpdate>();
            router.route(
                endpoint.path,
                put(
                    |state: State<AppState>,
                     path: std::result::Result<Path<usize>, PathRejection>,
                     payload: std::result::Result<
                        Json<<api_contracts::FileAnnotationsUpdate as ApiEndpointSpec>::Request>,
                        JsonRejection,
                    >| async move {
                        let response: handler_result_type!(api_contracts::FileAnnotationsUpdate) =
                            update_annotations_handler(state, path, payload).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::AnnotationsExport => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::AnnotationsExport>(endpoint);
            require_no_query::<api_contracts::AnnotationsExport>();
            require_no_request::<api_contracts::AnnotationsExport>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_result_type!(api_contracts::AnnotationsExport) =
                        export_annotations_handler(state).await;
                    response
                }),
            )
        }
    }
}

pub fn startup_event_json(server_url: &str, host: &str, port: u16) -> serde_json::Result<String> {
    serde_json::to_string(&StartupEvent {
        r#type: "server_started",
        url: server_url,
        host,
        port,
    })
}

async fn api_not_found_handler() -> ApiError {
    ApiError::not_found("API route not found")
}

async fn api_method_not_allowed_handler() -> ApiError {
    ApiError::method_not_allowed("method not allowed")
}

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    touch_request(&state);
    let status = state.registry.status();
    Json(HealthResponse {
        status: "ok",
        file_count: status.file_count,
        server_start_ms: state.server_start_ms,
    })
}

async fn files_handler(State(state): State<AppState>) -> Json<FilesResponse> {
    touch_request(&state);
    let status = state.registry.status();
    Json(FilesResponse {
        files: state.registry.summaries_snapshot(),
        tunnelled: state.tunnel_info.is_some(),
        tunnel_host: state.tunnel_info.as_ref().map(|t| t.tunnel_host.clone()),
        server_start_ms: state.server_start_ms,
        scan_complete: status.scan_complete,
        scanned: status.scanned,
        skipped: status.skipped,
        filtered: status.filtered,
    })
}

async fn info_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<usize>, PathRejection>,
) -> Result<Json<FrameInfo>, ApiError> {
    touch_request(&state);
    let Path(index) = path.map_err(api_path_rejection)?;
    let file = state
        .registry
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;
    Ok(Json(FrameInfo {
        frame_count: file.frame_count,
        rows: file.rows,
        columns: file.columns,
        transfer_syntax_uid: file.transfer_syntax_uid.clone(),
        has_pixels: file.has_pixels,
        default_window: file.default_window,
    }))
}

async fn annotations_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<usize>, PathRejection>,
) -> Result<Json<EmbedRoiAnnotations>, ApiError> {
    touch_request(&state);
    let Path(index) = path.map_err(api_path_rejection)?;
    if state.registry.get(index).is_none() {
        return Err(ApiError::not_found("file index out of range"));
    }

    let annotations = state
        .annotations
        .get(index)
        .map_err(|error| ApiError::internal(error.to_string()))?;

    Ok(Json(annotations))
}

async fn update_annotations_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<usize>, PathRejection>,
    payload: std::result::Result<Json<EmbedRoiAnnotations>, JsonRejection>,
) -> Result<Json<EmbedRoiAnnotations>, ApiError> {
    touch_request(&state);
    let Path(index) = path.map_err(api_path_rejection)?;
    let Json(annotations) = payload.map_err(api_json_rejection)?;
    let file = state
        .registry
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let canonical = state
        .annotations
        .replace_for_file(&file, annotations)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    Ok(Json(canonical))
}

async fn export_annotations_handler(State(state): State<AppState>) -> Result<Response, ApiError> {
    touch_request(&state);
    let files = state.registry.files_snapshot();
    let csv = state
        .annotations
        .export_embed_csv(files.as_slice())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut response = Response::new(axum::body::Body::from(csv));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        EXPORT_CONTENT_DISPOSITION_HEADER,
        HeaderValue::from_static(EXPORT_CONTENT_DISPOSITION_VALUE),
    );
    Ok(response)
}

async fn frame_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<(usize, u32)>, PathRejection>,
    query: std::result::Result<Query<FrameQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    touch_request(&state);
    let Path((index, frame)) = path.map_err(api_path_rejection)?;
    let Query(query) = query.map_err(api_query_rejection)?;
    let file = state
        .registry
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let frame_response = pixels::load_frame(
        file,
        state.pixel_cache.clone(),
        FrameRequest {
            frame,
            window_center: query.wc,
            window_width: query.ww,
            window_mode: query.mode.unwrap_or_default(),
        },
    )
    .await
    .map_err(pixel_error_to_api_error)?;

    let mut response = Response::new(axum::body::Body::from(frame_response.body));
    let cache_header = if frame_response.cache_hit {
        CACHE_HIT
    } else {
        CACHE_MISS
    };
    response
        .headers_mut()
        .insert(CACHE_HEADER, HeaderValue::from_static(cache_header));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(frame_response.content_type),
    );
    Ok(response)
}

async fn raw_frame_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<(usize, u32)>, PathRejection>,
) -> Result<Response, ApiError> {
    touch_request(&state);
    let Path((index, frame)) = path.map_err(api_path_rejection)?;
    let file = state
        .registry
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let raw_response =
        pixels::load_raw_frame(file, state.raw_cache.clone(), RawFrameRequest { frame })
            .await
            .map_err(pixel_error_to_api_error)?;

    let meta = &raw_response.metadata;
    let cache_header = if raw_response.cache_hit {
        CACHE_HIT
    } else {
        CACHE_MISS
    };

    let mut response = Response::new(axum::body::Body::from(raw_response.body));
    let headers = response.headers_mut();
    headers.insert(CACHE_HEADER, HeaderValue::from_static(cache_header));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    insert_header_if_valid(headers, RAW_FRAME_HEADER_ROWS, meta.rows.to_string());
    insert_header_if_valid(headers, RAW_FRAME_HEADER_COLUMNS, meta.columns.to_string());
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_BITS_ALLOCATED,
        meta.bits_allocated.to_string(),
    );
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_PIXEL_REPRESENTATION,
        meta.pixel_representation.to_string(),
    );
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_SAMPLES_PER_PIXEL,
        meta.samples_per_pixel.to_string(),
    );
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_PHOTOMETRIC_INTERPRETATION,
        meta.photometric_interpretation.clone(),
    );
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_RESCALE_SLOPE,
        meta.rescale_slope.to_string(),
    );
    insert_header_if_valid(
        headers,
        RAW_FRAME_HEADER_RESCALE_INTERCEPT,
        meta.rescale_intercept.to_string(),
    );
    if let Some(wc) = meta.default_wc {
        insert_header_if_valid(headers, RAW_FRAME_HEADER_DEFAULT_WC, wc.to_string());
    }
    if let Some(ww) = meta.default_ww {
        insert_header_if_valid(headers, RAW_FRAME_HEADER_DEFAULT_WW, ww.to_string());
    }

    Ok(response)
}

async fn tags_handler(
    State(state): State<AppState>,
    path: std::result::Result<Path<usize>, PathRejection>,
) -> Result<Json<Vec<TagNode>>, ApiError> {
    touch_request(&state);
    let Path(index) = path.map_err(api_path_rejection)?;
    let file = state
        .registry
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    if let Ok(cache) = state.tag_cache.lock() {
        if let Some(nodes) = cache.get(&index).cloned() {
            return Ok(Json(nodes));
        }
    }

    let path = file.path.clone();
    let nodes = tokio::task::spawn_blocking(move || build_tag_tree(&path))
        .await
        .map_err(|error| ApiError::internal(format!("tag serialization task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("tag serialization failed: {error}")))?;

    if let Ok(mut cache) = state.tag_cache.lock() {
        cache.insert(index, nodes.clone());
    }

    Ok(Json(nodes))
}

fn build_tag_tree(path: &std::path::Path) -> Result<Vec<TagNode>> {
    let object = open_file(path)
        .with_context(|| format!("failed to open DICOM for tags: {}", path.display()))?
        .into_inner();
    Ok(serialize_object_tags(&object, 0))
}

fn serialize_object_tags(
    object: &InMemDicomObject<StandardDataDictionary>,
    depth: usize,
) -> Vec<TagNode> {
    object
        .iter()
        .map(|element| serialize_element(element, depth))
        .collect()
}

fn serialize_element(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
    depth: usize,
) -> TagNode {
    let tag = element.header().tag;
    let tag_repr = format!("({:04X},{:04X})", tag.0, tag.1);
    let vr_repr = format!("{}", element.header().vr());
    let keyword = StandardDataDictionary
        .by_tag(tag)
        .map(|entry| entry.alias().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let value = serialize_tag_value(element, tag_repr.as_str(), &vr_repr, depth);

    TagNode {
        tag: tag_repr,
        vr: vr_repr,
        keyword,
        value,
    }
}

const TAG_TEXT_PREVIEW_LIMIT: usize = 256;
const TAG_NUMERIC_VALUE_LIMIT: usize = 128;
const TAG_SEQUENCE_ITEM_LIMIT: usize = 64;
const TAG_SEQUENCE_DEPTH_LIMIT: usize = 4;

fn serialize_tag_value(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
    tag_repr: &str,
    vr_repr: &str,
    depth: usize,
) -> TagValue {
    if tag_repr == "(7FE0,0010)" {
        return binary_value_from_element(element);
    }

    if vr_repr == "SQ" {
        return match element.items() {
            Some(items) => serialize_sequence_items(items, depth),
            None => TagValue::Error {
                message: "sequence item decoding failed".to_string(),
            },
        };
    }

    if matches!(vr_repr, "OB" | "OW" | "OD" | "OF" | "UN" | "OL") {
        return binary_value_from_element(element);
    }

    let string_value = match element.to_str() {
        Ok(value) => value.to_string(),
        Err(error) => {
            return TagValue::Error {
                message: format!("value serialization failed: {error}"),
            };
        }
    };

    if is_numeric_vr(vr_repr) {
        let mut numbers = string_value
            .split('\\')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect::<Vec<_>>();
        if numbers.is_empty() {
            TagValue::Error {
                message: "numeric conversion failed".to_string(),
            }
        } else if numbers.len() == 1 {
            TagValue::Number { value: numbers[0] }
        } else {
            let total = numbers.len();
            let truncated = total > TAG_NUMERIC_VALUE_LIMIT;
            numbers.truncate(TAG_NUMERIC_VALUE_LIMIT);
            TagValue::Numbers {
                value: numbers,
                truncated,
                total: truncated.then_some(total),
            }
        }
    } else {
        TagValue::String {
            value: format_tag_text_preview(&string_value),
        }
    }
}

fn serialize_sequence_items(
    items: &[InMemDicomObject<StandardDataDictionary>],
    depth: usize,
) -> TagValue {
    let total = items.len();
    let depth_limited = depth >= TAG_SEQUENCE_DEPTH_LIMIT;
    let item_limited = total > TAG_SEQUENCE_ITEM_LIMIT;

    if depth_limited {
        return TagValue::Sequence {
            items: Vec::new(),
            truncated: true,
            total: Some(total),
        };
    }

    let serialized_items = items
        .iter()
        .take(TAG_SEQUENCE_ITEM_LIMIT)
        .map(|item| serialize_object_tags(item, depth + 1))
        .collect();

    TagValue::Sequence {
        items: serialized_items,
        truncated: item_limited,
        total: item_limited.then_some(total),
    }
}

fn format_tag_text_preview(raw: &str) -> String {
    let normalized = raw.replace('\\', "; ");
    let mut chars = normalized.chars();
    let preview: String = chars.by_ref().take(TAG_TEXT_PREVIEW_LIMIT).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn binary_value_from_element(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
) -> TagValue {
    if let Some(length) = element.header().length().get() {
        return TagValue::Binary {
            length: length as usize,
        };
    }

    if let Some(fragments) = element.fragments() {
        let length = fragments.iter().map(|fragment| fragment.len()).sum();
        return TagValue::Binary { length };
    }

    match element.to_bytes() {
        Ok(bytes) => TagValue::Binary {
            length: bytes.len(),
        },
        Err(error) => TagValue::Error {
            message: format!("binary serialization failed: {error}"),
        },
    }
}

fn is_numeric_vr(vr_repr: &str) -> bool {
    matches!(
        vr_repr,
        "US" | "SS" | "UL" | "SL" | "FL" | "FD" | "DS" | "IS"
    )
}

async fn index_handler() -> impl IntoResponse {
    serve_asset("index.html").unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "frontend index asset missing".to_string(),
            }),
        )
            .into_response()
    })
}

async fn asset_handler(Path(path): Path<String>) -> impl IntoResponse {
    let full_path = format!("assets/{}", path.trim_start_matches('/'));
    serve_asset(&full_path).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("asset not found: {path}"),
            }),
        )
            .into_response()
    })
}

fn serve_asset(path: &str) -> Option<Response> {
    let normalized = path.trim_start_matches('/');
    let asset = FrontendAssets::get(normalized)?;
    let mime = match normalized.rsplit('.').next().unwrap_or_default() {
        "js" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };

    let mut response = Response::new(axum::body::Body::from(asset.data));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    Some(response)
}

fn insert_header_if_valid(headers: &mut HeaderMap, name: &'static str, value: String) {
    if let Ok(parsed) = HeaderValue::from_str(&value) {
        headers.insert(name, parsed);
    }
}

fn touch_request(state: &AppState) {
    state.last_request.store(now_unix_ms(), Ordering::Relaxed);
}

fn spawn_idle_timeout_watcher(
    timeout_seconds: u64,
    last_request: Arc<AtomicU64>,
    registry: FileRegistry,
    tunnel_handle: Option<Arc<TunnelHandle>>,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status = registry.status();
            if !status.scan_complete && status.file_count == 0 {
                continue;
            }
            let now = now_unix_ms();
            let last = last_request.load(Ordering::Relaxed);
            if last > 0 && now.saturating_sub(last) >= timeout_seconds * 1_000 {
                if let Some(handle) = &tunnel_handle {
                    handle.shutdown();
                }
                println!("dcmview: shutting down...");
                std::process::exit(0);
            }
        }
    });
}

fn spawn_browser_opener(server_url: String, registry: FileRegistry) {
    tokio::spawn(async move {
        loop {
            let changed = registry.changed();
            let mut changed = pin!(changed);
            changed.as_mut().enable();
            let status = registry.status();
            if status.file_count > 0 {
                if let Err(error) = open::that(&server_url) {
                    eprintln!("dcmview: warning — failed to open browser: {error}");
                }
                return;
            }
            if status.scan_complete {
                return;
            }
            changed.await;
        }
    });
}

async fn shutdown_signal(
    tunnel_handle: Option<Arc<TunnelHandle>>,
    shutdown_notify: Option<Arc<Notify>>,
) {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("ctrl+c handler");
    };

    #[cfg(windows)]
    let ctrl_break = async {
        let mut signal = tokio::signal::windows::ctrl_break().expect("ctrl+break handler");
        signal.recv().await;
    };

    #[cfg(not(windows))]
    let ctrl_break = std::future::pending::<()>();

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    if let Some(shutdown_notify) = shutdown_notify {
        tokio::select! {
            _ = ctrl_c => {},
            _ = ctrl_break => {},
            _ = terminate => {},
            _ = shutdown_notify.notified() => {},
        }
    } else {
        tokio::select! {
            _ = ctrl_c => {},
            _ = ctrl_break => {},
            _ = terminate => {},
        }
    }

    if let Some(handle) = tunnel_handle {
        handle.shutdown();
    }
    println!("dcmview: shutting down...");
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn from_rejection(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

fn api_path_rejection(error: PathRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

fn api_query_rejection(error: QueryRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

fn api_json_rejection(error: JsonRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

fn pixel_error_to_api_error(error: pixels::PixelError) -> ApiError {
    match error {
        pixels::PixelError::NoPixelData | pixels::PixelError::FrameOutOfRange => {
            ApiError::not_found(error.to_string())
        }
        pixels::PixelError::InvalidWindow(_) => ApiError::bad_request(error.to_string()),
        pixels::PixelError::UnsupportedTransferSyntax(_)
        | pixels::PixelError::UnsupportedLayout(_) => ApiError::unprocessable(error.to_string()),
        pixels::PixelError::Decode { .. } => ApiError::internal(error.to_string()),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::is_non_loopback_bind;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn detects_loopback_and_non_loopback_bind_addresses() {
        assert!(!is_non_loopback_bind(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(!is_non_loopback_bind(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_non_loopback_bind(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(is_non_loopback_bind(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(is_non_loopback_bind(IpAddr::V4(Ipv4Addr::new(
            192, 168, 1, 24
        ))));
    }
}
