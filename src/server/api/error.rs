use crate::api::contracts::ErrorResponse;
use crate::pixels::{self, PixelError};
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug)]
pub(super) struct ApiError {
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

    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
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

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

pub(super) fn path_rejection(error: PathRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

pub(super) fn query_rejection(error: QueryRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

pub(super) fn json_rejection(error: JsonRejection) -> ApiError {
    ApiError::from_rejection(error.status(), error.body_text())
}

pub(super) fn pixel_error(error: PixelError) -> ApiError {
    match error {
        pixels::PixelError::NoPixelData | pixels::PixelError::FrameOutOfRange => {
            ApiError::not_found(error.to_string())
        }
        pixels::PixelError::InvalidWindow(_) => ApiError::bad_request(error.to_string()),
        pixels::PixelError::UnsupportedTransferSyntax(_)
        | pixels::PixelError::UnsupportedLayout(_) => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: error.to_string(),
        },
        pixels::PixelError::Decode { .. } => ApiError::internal(error.to_string()),
    }
}

pub(super) async fn not_found_handler() -> ApiError {
    ApiError::not_found("API route not found")
}

pub(super) async fn method_not_allowed_handler() -> ApiError {
    ApiError::method_not_allowed("method not allowed")
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
