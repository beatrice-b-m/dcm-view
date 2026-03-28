use crate::pixels::{self, FrameRequest};
use crate::tunnel::{self, TunnelHandle};
use crate::types::{ErrorResponse, FileEntry, FileSummary, FilesResponse, FrameInfo, TagNode, TagValue, TunnelInfo};
use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bytes::Bytes;
use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_dictionary_std::StandardDataDictionary;
use dicom_object::{open_file, InMemDicomObject};
use lru::LruCache;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct AppState {
	pub files: Arc<Vec<FileEntry>>,
	pub pixel_cache: Arc<Mutex<LruCache<crate::types::FrameCacheKey, Bytes>>>,
	pub tag_cache: Arc<Mutex<HashMap<usize, Vec<TagNode>>>>,
	pub tunnel_info: Option<Arc<TunnelInfo>>,
	pub tunnel_handle: Option<Arc<TunnelHandle>>,
	pub server_start: Instant,
	pub server_start_ms: u64,
	pub last_request: Arc<AtomicU64>,
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
	pub tunnel: Option<TunnelConfig>,
}

#[derive(Debug, Deserialize)]
struct FrameQuery {
	wc: Option<f64>,
	ww: Option<f64>,
}

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

pub async fn run(config: ServerConfig, mut state: AppState) -> Result<()> {
	let bind_addr = format!("{}:{}", config.host, config.port);
	let listener = TcpListener::bind(&bind_addr)
		.await
		.with_context(|| format!("failed to bind to {bind_addr}"))?;
	let local_addr = listener.local_addr().context("failed to read local bind address")?;
	let server_url = format!("http://{}:{}", local_addr.ip(), local_addr.port());

	println!("dcmview: server running at {server_url}");

	if let Some(tunnel_cfg) = config.tunnel {
		let runtime = tunnel::start_tunnel(local_addr.port(), tunnel_cfg.host.clone(), tunnel_cfg.port)?;
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
		if let Err(error) = open::that(&server_url) {
			eprintln!("dcmview: warning — failed to open browser: {error}");
		}
	}

	println!("dcmview: press Ctrl+C to stop");

	if let Some(timeout) = config.timeout_seconds {
		spawn_idle_timeout_watcher(timeout, state.last_request.clone(), state.tunnel_handle.clone());
	}

	let tunnel_handle = state.tunnel_handle.clone();
	let app = router(state);
	axum::serve(listener, app)
		.with_graceful_shutdown(shutdown_signal(tunnel_handle))
		.await
		.context("server failed")
}

pub fn now_unix_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or(Duration::from_secs(0))
		.as_millis() as u64
}

pub fn router(state: AppState) -> Router {
	Router::new()
		.route("/", get(index_handler))
		.route("/assets/{*path}", get(asset_handler))
		.route("/api/files", get(files_handler))
		.route("/api/file/{index}/info", get(info_handler))
		.route("/api/file/{index}/frame/{frame}", get(frame_handler))
		.route("/api/file/{index}/tags", get(tags_handler))
		.with_state(state)
}

async fn files_handler(State(state): State<AppState>) -> Json<FilesResponse> {
	touch_request(&state);
	let files = state.files.iter().map(FileSummary::from).collect();
	Json(FilesResponse {
		files,
		tunnelled: state.tunnel_info.is_some(),
		tunnel_host: state.tunnel_info.as_ref().map(|t| t.tunnel_host.clone()),
		server_start_ms: state.server_start_ms,
	})
}

async fn info_handler(State(state): State<AppState>, Path(index): Path<usize>) -> Result<Json<FrameInfo>, ApiError> {
	touch_request(&state);
	let file = state
		.files
		.get(index)
		.ok_or_else(|| ApiError::not_found("file index out of range"))?;
	Ok(Json(FrameInfo {
		frame_count: file.frame_count,
		rows: file.rows,
		columns: file.columns,
		transfer_syntax: file.transfer_syntax_uid.clone(),
		has_pixels: file.has_pixels,
		default_window: file.default_window,
	}))
}

async fn frame_handler(
	State(state): State<AppState>,
	Path((index, frame)): Path<(usize, u32)>,
	Query(query): Query<FrameQuery>,
	headers: HeaderMap,
) -> Result<Response, ApiError> {
	touch_request(&state);
	let accept_header = headers
		.get(header::ACCEPT)
		.and_then(|value| value.to_str().ok())
		.map(ToString::to_string);

	let frame_response = pixels::load_frame(
		state.files.as_slice(),
		state.pixel_cache.clone(),
		FrameRequest {
			file_index: index,
			frame,
			window_center: query.wc,
			window_width: query.ww,
			accept_header,
		},
	)
	.await
	.map_err(|err| {
		let message = err.to_string();
		if message.contains("unsupported transfer syntax") {
			ApiError::unprocessable(message)
		} else if message.contains("no pixel data") || message.contains("frame out of range") {
			ApiError::not_found(message)
		} else {
			ApiError::internal(format!("frame decode failed: {message}"))
		}
	})?;

	let mut response = Response::new(axum::body::Body::from(frame_response.body));
	let cache_header = if frame_response.cache_hit { "HIT" } else { "MISS" };
	response
		.headers_mut()
		.insert("X-Cache", cache_header.parse().expect("valid cache header"));
	response.headers_mut().insert(
		header::CONTENT_TYPE,
		frame_response
			.content_type
			.parse()
			.expect("valid content type header"),
	);
	Ok(response)
}

async fn tags_handler(State(state): State<AppState>, Path(index): Path<usize>) -> Result<Json<Vec<TagNode>>, ApiError> {
	touch_request(&state);
	let file = state
		.files
		.get(index)
		.ok_or_else(|| ApiError::not_found("file index out of range"))?;

	if let Ok(cache) = state.tag_cache.lock() {
		if let Some(nodes) = cache.get(&index).cloned() {
			return Ok(Json(nodes));
		}
	}

	let nodes = build_tag_tree(&file.path)
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
	Ok(serialize_object_tags(&object))
}

fn serialize_object_tags(object: &InMemDicomObject<StandardDataDictionary>) -> Vec<TagNode> {
	object.iter().map(serialize_element).collect()
}

fn serialize_element(
	element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
) -> TagNode {
	let tag = element.header().tag;
	let tag_repr = format!("({:04X},{:04X})", tag.0, tag.1);
	let vr_repr = format!("{}", element.header().vr());
	let keyword = StandardDataDictionary
		.by_tag(tag)
		.map(|entry| entry.alias().to_string())
		.unwrap_or_else(|| "Unknown".to_string());

	let value = serialize_tag_value(element, tag_repr.as_str(), &vr_repr);

	TagNode {
		tag: tag_repr,
		vr: vr_repr,
		keyword,
		value,
	}
}

fn serialize_tag_value(
	element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
	tag_repr: &str,
	vr_repr: &str,
) -> TagValue {
	if tag_repr == "(7FE0,0010)" {
		return binary_value_from_element(element);
	}

	if vr_repr == "SQ" {
		return match element.items() {
			Some(items) => TagValue::Sequence {
				items: items.iter().map(serialize_object_tags).collect(),
			},
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
		let numbers = string_value
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
			TagValue::Numbers { value: numbers }
		}
	} else {
		let text = if string_value.len() > 256 {
			format!("{}…", &string_value[..256])
		} else {
			string_value.replace('\\', "; ")
		};
		TagValue::String { value: text }
	}
}

fn binary_value_from_element(element: &dicom_object::mem::InMemElement<StandardDataDictionary>) -> TagValue {
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
	matches!(vr_repr, "US" | "SS" | "UL" | "SL" | "FL" | "FD" | "DS" | "IS")
}

async fn index_handler() -> impl IntoResponse {
	serve_asset("index.html").unwrap_or_else(|| {
		(StatusCode::NOT_FOUND, Json(ErrorResponse {
			error: "frontend index asset missing".to_string(),
		}))
			.into_response()
	})
}

async fn asset_handler(Path(path): Path<String>) -> impl IntoResponse {
	let full_path = format!("assets/{}", path.trim_start_matches('/'));
	serve_asset(&full_path).unwrap_or_else(|| {
		(StatusCode::NOT_FOUND, Json(ErrorResponse {
			error: format!("asset not found: {path}"),
		}))
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
		.insert(header::CONTENT_TYPE, mime.parse().expect("valid mime"));
	Some(response)
}

fn touch_request(state: &AppState) {
	state.last_request.store(now_unix_ms(), Ordering::Relaxed);
}

fn spawn_idle_timeout_watcher(
	timeout_seconds: u64,
	last_request: Arc<AtomicU64>,
	tunnel_handle: Option<Arc<TunnelHandle>>,
) {
	tokio::spawn(async move {
		loop {
			tokio::time::sleep(Duration::from_secs(1)).await;
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

async fn shutdown_signal(tunnel_handle: Option<Arc<TunnelHandle>>) {
	let ctrl_c = async {
		tokio::signal::ctrl_c().await.expect("ctrl+c handler");
	};

	#[cfg(unix)]
	let terminate = async {
		tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
			.expect("sigterm handler")
			.recv()
			.await;
	};

	#[cfg(not(unix))]
	let terminate = std::future::pending::<()>();

	tokio::select! {
		_ = ctrl_c => {},
		_ = terminate => {},
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
	fn not_found(message: impl Into<String>) -> Self {
		Self {
			status: StatusCode::NOT_FOUND,
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

impl IntoResponse for ApiError {
	fn into_response(self) -> Response {
		(self.status, Json(ErrorResponse { error: self.message })).into_response()
	}
}
