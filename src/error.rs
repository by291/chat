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

    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", "没有执行此操作的权限")
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
    fn from(error: sqlx::Error) -> Self {
        // SQLSTATE and typed categories identify failures without logging SQL,
        // parameters, constraint details or rows containing private capabilities.
        let sql_state = error.as_database_error().and_then(|error| error.code());
        let error_kind = match &error {
            sqlx::Error::Database(_) => "database",
            sqlx::Error::ColumnNotFound(_) => "column_not_found",
            sqlx::Error::ColumnDecode { .. } => "column_decode",
            sqlx::Error::Decode(_) => "decode",
            sqlx::Error::PoolTimedOut => "pool_timed_out",
            sqlx::Error::PoolClosed => "pool_closed",
            _ => "other",
        };
        tracing::error!(
            category = "database",
            sql_state = sql_state.as_deref(),
            error_kind,
            "database operation failed"
        );
        Self::unavailable()
    }
}
