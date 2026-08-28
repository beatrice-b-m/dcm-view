use super::error::{self, ApiError};
use super::state::AppState;
use crate::api::contracts::{
    DiscoveryResult, EmbedRoiAnnotations, FileSummary, FilesResponse, FrameInfo, FrameQuery,
    HealthResponse, SeriesCatalogResponse, TagNode, TagQuery, ViewerIdentity, CACHE_HEADER,
    CACHE_HIT, CACHE_MISS, EXPORT_CONTENT_DISPOSITION_HEADER, EXPORT_CONTENT_DISPOSITION_VALUE,
    RAW_FRAME_HEADER_BITS_ALLOCATED, RAW_FRAME_HEADER_COLUMNS, RAW_FRAME_HEADER_DEFAULT_WC,
    RAW_FRAME_HEADER_DEFAULT_WW, RAW_FRAME_HEADER_PHOTOMETRIC_INTERPRETATION,
    RAW_FRAME_HEADER_PIXEL_REPRESENTATION, RAW_FRAME_HEADER_RESCALE_INTERCEPT,
    RAW_FRAME_HEADER_RESCALE_SLOPE, RAW_FRAME_HEADER_ROWS, RAW_FRAME_HEADER_SAMPLES_PER_PIXEL,
};
use crate::pixels::{self, FrameRequest, RawFrameRequest};
use crate::server::tags;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::Response;
use axum::Json;

pub(super) async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let status = state.registry().status();
    Json(HealthResponse {
        status: "ok",
        viewer: ViewerIdentity::current(),
        file_count: status.file_count,
        server_start_ms: state.server_start_ms(),
    })
}

pub(super) async fn files(State(state): State<AppState>) -> Json<FilesResponse> {
    let status = state.registry().status();
    let tunnel = state.tunnel_info();
    Json(FilesResponse {
        files: state.registry().summaries_snapshot(),
        discovery: state
            .registry()
            .discovery_ledger_snapshot()
            .into_iter()
            .map(|record| DiscoveryResult {
                path: record.path.display().to_string(),
                disposition: match record.disposition {
                    crate::loader::DiscoveryDisposition::Selected => "selected",
                    crate::loader::DiscoveryDisposition::Skipped => "skipped",
                    crate::loader::DiscoveryDisposition::Filtered => "filtered",
                }
                .to_string(),
                reason: record.reason.code().to_string(),
            })
            .collect(),
        tunnelled: tunnel.is_some(),
        tunnel_host: tunnel.map(|info| info.tunnel_host.clone()),
        server_start_ms: state.server_start_ms(),
        scan_complete: status.scan_complete,
        scanned: status.scanned,
        skipped: status.skipped,
        filtered: status.filtered,
    })
}

pub(super) async fn series(State(state): State<AppState>) -> Json<SeriesCatalogResponse> {
    Json(state.registry().series_catalog_snapshot())
}

pub(super) async fn info(
    State(state): State<AppState>,
    path: Result<Path<usize>, PathRejection>,
) -> Result<Json<FrameInfo>, ApiError> {
    let Path(index) = path.map_err(error::path_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;
    let summary = FileSummary::from(&file);
    Ok(Json(FrameInfo {
        frame_count: file.frame_count,
        rows: file.rows,
        columns: file.columns,
        transfer_syntax_uid: file.transfer_syntax_uid.clone(),
        has_pixels: file.has_pixels,
        sop_class_uid: summary.sop_class_uid,
        object_kind: summary.object_kind,
        support_state: summary.support_state,
        support_reason: summary.support_reason,
        default_window: file.default_window,
    }))
}

pub(super) async fn annotations(
    State(state): State<AppState>,
    path: Result<Path<usize>, PathRejection>,
) -> Result<Json<EmbedRoiAnnotations>, ApiError> {
    let Path(index) = path.map_err(error::path_rejection)?;
    if state.registry().get(index).is_none() {
        return Err(ApiError::not_found("file index out of range"));
    }

    state
        .annotations()
        .wait_until_ready()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let annotations = state
        .annotations()
        .get(index)
        .map_err(|error| ApiError::internal(error.to_string()))?;

    Ok(Json(annotations))
}

pub(super) async fn update_annotations(
    State(state): State<AppState>,
    path: Result<Path<usize>, PathRejection>,
    payload: Result<Json<EmbedRoiAnnotations>, JsonRejection>,
) -> Result<Json<EmbedRoiAnnotations>, ApiError> {
    let Path(index) = path.map_err(error::path_rejection)?;
    let Json(annotations) = payload.map_err(error::json_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let canonical = state
        .annotations()
        .replace_for_file(&file, annotations)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    Ok(Json(canonical))
}

pub(super) async fn export_annotations(
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    state
        .annotations()
        .wait_until_ready()
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let files = state.registry().files_snapshot();
    let csv = state
        .annotations()
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

pub(super) async fn frame(
    State(state): State<AppState>,
    path: Result<Path<(usize, u32)>, PathRejection>,
    query: Result<Query<FrameQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Path((index, frame)) = path.map_err(error::path_rejection)?;
    let Query(query) = query.map_err(error::query_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let frame_response = pixels::load_frame(
        file,
        state.pixel_cache(),
        FrameRequest {
            frame,
            window_center: query.wc,
            window_width: query.ww,
            window_mode: query.mode.unwrap_or_default(),
        },
    )
    .await
    .map_err(error::pixel_error)?;

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

pub(super) async fn raw_frame(
    State(state): State<AppState>,
    path: Result<Path<(usize, u32)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((index, frame)) = path.map_err(error::path_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    let raw_response = pixels::load_raw_frame(file, state.raw_cache(), RawFrameRequest { frame })
        .await
        .map_err(error::pixel_error)?;

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

pub(super) async fn tags(
    State(state): State<AppState>,
    path: Result<Path<usize>, PathRejection>,
) -> Result<Json<Vec<TagNode>>, ApiError> {
    let Path(index) = path.map_err(error::path_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;

    if let Some(nodes) = state.cached_tags(index) {
        return Ok(Json(nodes));
    }

    let path = file.path.clone();
    let nodes = tokio::task::spawn_blocking(move || tags::build_tag_tree(&path))
        .await
        .map_err(|error| ApiError::internal(format!("tag serialization task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("tag serialization failed: {error}")))?;

    state.cache_tags(index, nodes.clone());
    Ok(Json(nodes))
}

pub(super) async fn select_tag(
    State(state): State<AppState>,
    path: Result<Path<usize>, PathRejection>,
    query: Result<Query<TagQuery>, QueryRejection>,
) -> Result<Json<TagNode>, ApiError> {
    let Path(index) = path.map_err(error::path_rejection)?;
    let Query(query) = query.map_err(error::query_rejection)?;
    let file = state
        .registry()
        .get(index)
        .ok_or_else(|| ApiError::not_found("file index out of range"))?;
    let path = file.path.clone();
    let selector = query.path;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(tags::TAG_SELECT_DEFAULT_LIMIT);
    let node = tokio::task::spawn_blocking(move || {
        tags::build_selected_tag(&path, &selector, offset, limit)
    })
    .await
    .map_err(|error| ApiError::internal(format!("tag selection task failed: {error}")))?
    .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(node))
}

fn insert_header_if_valid(headers: &mut HeaderMap, name: &'static str, value: String) {
    if let Ok(parsed) = HeaderValue::from_str(&value) {
        headers.insert(name, parsed);
    }
}
