use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::{
    FileEntry, FrameCacheKey, RawFrameCacheKey, TransferSyntaxClass, WindowRequest,
};
use bytes::Bytes;
use std::sync::{Arc, Mutex};

use super::cache::{FrameCache, RawFrameCache, FRAME_CACHE_MAX_BYTES, RAW_CACHE_MAX_BYTES};
use super::error::{PixelError, PixelResult};
use super::jpeg::{
    decode_compressed_frame_to_png, decode_raw_jpeg_lossless, read_raw_jpeg_samples,
};
use super::jpeg2000::{decode_jp2_fragment_to_png, decode_raw_jp2_samples};
use super::native::{decode_uncompressed_to_png, read_raw_uncompressed};
use super::syntax::classify_transfer_syntax;

#[derive(Debug, Clone)]
pub struct RawFrameRequest {
    pub frame: u32,
}

#[derive(Debug, Clone)]
pub struct RawFrameResponse {
    pub body: Bytes,
    pub metadata: RawFrameMetadata,
    pub cache_hit: bool,
}

pub async fn load_raw_frame(
    file: FileEntry,
    cache: Arc<Mutex<RawFrameCache>>,
    request: RawFrameRequest,
) -> PixelResult<RawFrameResponse> {
    if !file.has_pixels {
        return Err(PixelError::NoPixelData);
    }
    if request.frame >= file.frame_count {
        return Err(PixelError::FrameOutOfRange);
    }

    let syntax_class = classify_transfer_syntax(&file.transfer_syntax_uid);
    if matches!(
        syntax_class,
        TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported
    ) {
        return Err(PixelError::UnsupportedTransferSyntax(
            file.transfer_syntax_uid.clone(),
        ));
    }

    let key = RawFrameCacheKey {
        file_index: file.index,
        frame: request.frame,
    };

    if let Ok(mut lock) = cache.lock() {
        if let Some((bytes, meta)) = lock.get(&key) {
            return Ok(RawFrameResponse {
                body: bytes,
                metadata: meta,
                cache_hit: true,
            });
        }
    }

    let (body, metadata) = match syntax_class {
        TransferSyntaxClass::Jpeg => read_raw_jpeg_samples(file.clone(), request.frame)
            .await
            .map_err(PixelError::raw_decode)?,
        TransferSyntaxClass::JpegLossless => {
            decode_raw_jpeg_lossless(file.clone(), request.frame).await?
        }
        TransferSyntaxClass::Jpeg2000 => {
            decode_raw_jp2_samples(file.clone(), request.frame).await?
        }
        TransferSyntaxClass::Uncompressed => read_raw_uncompressed(file.clone(), request.frame)
            .await
            .map_err(PixelError::raw_decode)?,
        _ => unreachable!("non-raw syntaxes filtered above"),
    };

    if let Ok(mut lock) = cache.lock() {
        lock.insert_with_budget(key, body.clone(), metadata.clone(), RAW_CACHE_MAX_BYTES);
    }

    Ok(RawFrameResponse {
        body,
        metadata,
        cache_hit: false,
    })
}

#[derive(Debug, Clone)]
pub struct FrameRequest {
    pub frame: u32,
    pub window_center: Option<f64>,
    pub window_width: Option<f64>,
    pub window_mode: WindowMode,
}

#[derive(Debug, Clone)]
pub struct FrameResponse {
    pub body: Bytes,
    pub content_type: &'static str,
    pub cache_hit: bool,
}

pub async fn load_frame(
    file: FileEntry,
    cache: Arc<Mutex<FrameCache>>,
    request: FrameRequest,
) -> PixelResult<FrameResponse> {
    if !file.has_pixels {
        return Err(PixelError::NoPixelData);
    }
    if request.frame >= file.frame_count {
        return Err(PixelError::FrameOutOfRange);
    }

    let window = WindowRequest::new(
        request.window_center,
        request.window_width,
        request.window_mode,
    )
    .map_err(|error| PixelError::InvalidWindow(error.to_string()))?;
    let syntax_class = classify_transfer_syntax(&file.transfer_syntax_uid);
    let key = FrameCacheKey::new(
        file.index,
        request.frame,
        window.center(),
        window.width(),
        window.mode(),
    );

    if let Ok(mut lock) = cache.lock() {
        if let Some(bytes) = lock.get(&key) {
            return Ok(FrameResponse {
                body: bytes,
                content_type: "image/png",
                cache_hit: true,
            });
        }
    }

    let (body, content_type) = match syntax_class {
        TransferSyntaxClass::Jpeg => (
            decode_compressed_frame_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::JpegLossless => (
            decode_compressed_frame_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::Jpeg2000 => (
            decode_jp2_fragment_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::Uncompressed => (
            decode_uncompressed_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::JpegLs
        | TransferSyntaxClass::Rle
        | TransferSyntaxClass::Unsupported => {
            return Err(PixelError::UnsupportedTransferSyntax(
                file.transfer_syntax_uid.clone(),
            ));
        }
    };

    if let Ok(mut lock) = cache.lock() {
        lock.insert_with_budget(key, body.clone(), FRAME_CACHE_MAX_BYTES);
    }

    Ok(FrameResponse {
        body,
        content_type,
        cache_hit: false,
    })
}
