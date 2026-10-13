//! Shared credential gate for listeners reachable only by trusted infrastructure.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

use crate::{AppState, error::AppError};

pub(crate) async fn credential(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let provided = request
        .headers()
        .get("x-proxy-key")
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    if !bool::from(provided.ct_eq(state.config.internal_proxy_key.as_bytes())) {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "internal_only",
            "内部接口",
        ));
    }
    Ok(next.run(request).await)
}
