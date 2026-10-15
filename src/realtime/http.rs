//! HTTP adapters for private Centrifugo callbacks and metrics.

use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};

use crate::{AppState, error::AppError};

pub use super::model::{
    ConnectMeta, ConnectReply, ConnectResult, Disconnect, ProxyConnectRequest, ProxyRefreshRequest,
    RefreshReply, RefreshResult, SessionMeta,
};

pub use super::service::refresh_proxy;
pub(crate) use crate::infrastructure::internal_auth::credential;

/// Extract forwarded transport inputs before invoking application authorization.
pub async fn connect_proxy(
    state: &AppState,
    input: ProxyConnectRequest,
) -> Result<ConnectReply, AppError> {
    let origin = input
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("origin"))
        .map(|(_, value)| value.as_str());

    let mut headers = HeaderMap::new();
    if let Some((_, cookie)) = input
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        && let Ok(value) = cookie.parse()
    {
        headers.insert("cookie", value);
    }
    let token = crate::auth::http::cookie_token(&headers, state);

    super::service::connect_proxy(state, &input.client, origin, token.as_deref()).await
}

async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut input): Json<ProxyConnectRequest>,
) -> Result<Json<ConnectReply>, AppError> {
    // Centrifugo forwards transport headers as HTTP headers, never as the JSON connect payload.
    for key in ["cookie", "origin"] {
        if let Some(value) = headers.get(key).and_then(|v| v.to_str().ok()) {
            input.headers.insert(key.into(), value.into());
        }
    }

    Ok(Json(connect_proxy(&state, input).await?))
}

async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<ProxyRefreshRequest>,
) -> Result<Json<RefreshReply>, AppError> {
    Ok(Json(refresh_proxy(&state, input).await?))
}

/// Compose the proxy and metrics routes behind the private credential gate.
pub fn build_internal_router(state: AppState) -> Router {
    Router::new()
        .route("/internal/centrifugo/connect", post(connect))
        .route("/internal/centrifugo/refresh", post(refresh))
        .route("/metrics", axum::routing::get(metrics))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            credential,
        ))
        .with_state(state)
        .layer(axum::middleware::from_fn(
            crate::infrastructure::observability::http,
        ))
}

async fn metrics(State(state): State<AppState>) -> Result<axum::response::Response, AppError> {
    let backlog = super::service::outbox_stats(&state).await?;

    Ok(crate::infrastructure::observability::render_metrics(
        backlog.pending,
        backlog.oldest_age_seconds,
    ))
}

/// Compose the worker's metrics-only listener behind the internal credential gate.
pub fn metrics_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", axum::routing::get(metrics))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            credential,
        ))
        .with_state(state)
}
