//! Request correlation, bounded-cardinality metrics, and process shutdown signals.

use std::{sync::OnceLock, time::Instant};

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing::Instrument;

use crate::error::AppError;

tokio::task_local! {
    static REQUEST_ID: String;
}

static METRICS: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install one process-wide recorder before serving any application or worker routes.
pub fn init() {
    METRICS.get_or_init(|| {
        // OnceLock serializes initialization; a second recorder is a wiring bug.
        PrometheusBuilder::new()
            .install_recorder()
            .expect("single metrics recorder")
    });
}

/// Return the current request's correlation ID, or a new ID outside HTTP handling.
pub fn request_id() -> String {
    REQUEST_ID
        .try_with(Clone::clone)
        .unwrap_or_else(|_| uuid::Uuid::now_v7().to_string())
}

/// Correlate errors and logs while recording only route templates, never user paths.
pub async fn http(request: Request, next: Next) -> Response {
    let id = uuid::Uuid::now_v7().to_string();
    let method = request.method().as_str().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or("unmatched".into());
    let started = Instant::now();
    let span = tracing::info_span!("http_request", request_id=%id, method=%method, route=%route);
    // A generated UUID contains only ASCII hex and hyphens, which are valid header bytes.
    let header = id
        .parse::<HeaderValue>()
        .expect("UUID is a valid header value");

    let mut response = REQUEST_ID
        .scope(
            id,
            async move {
                let response = next.run(request).await;
                let response = normalize_extractor_error(response);
                let elapsed = started.elapsed().as_secs_f64();

                metrics::histogram!(
                    "chat_http_request_duration_seconds",
                    "method" => method,
                    "route" => route,
                    "status" => response.status().as_u16().to_string()
                )
                .record(elapsed);
                tracing::info!(
                    status = response.status().as_u16(),
                    duration_ms = elapsed * 1000.,
                    "request completed"
                );
                response
            }
            .instrument(span),
        )
        .await;

    response.headers_mut().insert("x-request-id", header);
    response
}

fn normalize_extractor_error(response: Response) -> Response {
    let is_json = response
        .headers()
        .get("content-type")
        .is_some_and(|value| value.as_bytes().starts_with(b"application/json"));
    if !response.status().is_client_error() || is_json {
        return response;
    }

    // Framework rejections otherwise expose inconsistent plaintext or extractor
    // details. Keep the same public error envelope as application operations.
    let error = match response.status() {
        StatusCode::NOT_FOUND => AppError::not_found(),
        StatusCode::METHOD_NOT_ALLOWED => AppError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "请求方法不受支持",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "请求内容过大",
        ),
        _ => AppError::invalid("请求格式无效"),
    };
    error.into_response()
}

/// Render process metrics from application-supplied backlog values.
pub fn render_metrics(pending: i64, oldest_age_seconds: f64) -> Response {
    metrics::gauge!("chat_outbox_pending").set(pending as f64);
    metrics::gauge!("chat_outbox_oldest_age_seconds").set(oldest_age_seconds.max(0.));

    // AppState initializes the recorder before any metrics route can be built.
    let rendered = METRICS.get().expect("metrics initialized").render();
    ([("content-type", "text/plain; version=0.0.4")], rendered).into_response()
}

/// Wait for a termination signal or an operator-owned maintenance marker.
/// Native Windows backups use the marker to drain tasks without force-killing writers.
pub async fn shutdown_signal() {
    let marker = async {
        if let Some(path) = std::env::var_os("CHAT_SHUTDOWN_FILE") {
            wait_for_shutdown_marker(std::path::PathBuf::from(path)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = platform_shutdown_signal() => {},
        _ = marker => {},
    }
}

async fn wait_for_shutdown_marker(path: std::path::PathBuf) {
    loop {
        match tokio::fs::try_exists(&path).await {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "maintenance marker check failed"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn platform_shutdown_signal() {
    #[cfg(unix)]
    {
        // Installing the handler is a startup invariant; failure prevents graceful shutdown.
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "shutdown signal handler failed");
        }
    }
}

#[cfg(test)]
mod shutdown_tests {
    #[tokio::test]
    async fn marker_shutdown_waits_until_the_owned_file_exists() {
        let marker = std::env::temp_dir().join(format!("chat-shutdown-{}", uuid::Uuid::new_v4()));
        let waiter = tokio::spawn(super::wait_for_shutdown_marker(marker.clone()));
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        tokio::fs::write(&marker, b"maintenance").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();

        tokio::fs::remove_file(&marker).await.unwrap();
    }
}
