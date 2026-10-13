//! Stable public errors that omit sensitive dependency details.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: &'static str,
}

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }

    pub fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "dependency_unavailable",
            "服务暂时不可用，请稍后重试",
        )
    }

    pub fn invalid(message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_input", message)
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "请重新登录")
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "资源不存在")
    }

    pub fn conflict(message: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let request_id = crate::infrastructure::observability::request_id();
        tracing::warn!(%request_id, code = self.code, status = self.status.as_u16(), "request rejected");
        (
            self.status,
            Json(ApiErrorBody {
                code: self.code.into(),
                message: self.message.into(),
                request_id,
            }),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(_error: sqlx::Error) -> Self {
        // Do not log SQL or connection parameters, which may contain private data.
        tracing::error!(category = "database", "database operation failed");
        Self::unavailable()
    }
}
