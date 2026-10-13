//! Authenticated media transport adapters with stable metadata and ephemeral grants.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};

use super::{
    model::{
        AttachmentView, CompleteInput, DownloadGrant, DownloadQuery, DownloadVariant, UploadGrant,
        UploadInput,
    },
    service,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, AttachmentId},
};

/// Allocate an upload scoped to the current actor and membership episode.
#[utoipa::path(post, path = "/api/v1/media/uploads", operation_id = "media_create_upload", request_body = UploadInput, responses((status = 201, body = UploadGrant)))]
pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<UploadInput>,
) -> Result<Response, AppError> {
    Ok(no_store((
        StatusCode::CREATED,
        Json(service::create_upload(&state, actor, input).await?),
    )))
}

/// Renew a grant with a new independently reserved staging attempt.
#[utoipa::path(post, path = "/api/v1/media/{id}/upload-url", operation_id = "media_renew_upload", params(("id" = AttachmentId, Path)), responses((status = 200, body = UploadGrant)))]
pub async fn renew(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<AttachmentId>,
) -> Result<Response, AppError> {
    Ok(no_store(Json(
        service::renew_upload(&state, actor, id).await?,
    )))
}

/// Seal the selected attempt and enqueue processing idempotently.
#[utoipa::path(post, path = "/api/v1/media/{id}/complete", operation_id = "media_complete_upload", params(("id" = AttachmentId, Path)), request_body = CompleteInput, responses((status = 200, body = AttachmentView)))]
pub async fn complete(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<AttachmentId>,
    Json(input): Json<CompleteInput>,
) -> Result<Json<AttachmentView>, AppError> {
    Ok(Json(
        service::complete_upload(&state, actor, id, input).await?,
    ))
}

/// Inspect owned processing state or an authorized visible reference.
#[utoipa::path(get, path = "/api/v1/media/{id}", operation_id = "media_get_attachment", params(("id" = AttachmentId, Path)), responses((status = 200, body = AttachmentView)))]
pub async fn get(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<AttachmentId>,
) -> Result<Json<AttachmentView>, AppError> {
    Ok(Json(service::get_attachment(&state, actor, id).await?))
}

/// Cancel an unreferenced upload, preserving outstanding PUT deadlines for cleanup.
#[utoipa::path(delete, path = "/api/v1/media/{id}", operation_id = "media_cancel_upload", params(("id" = AttachmentId, Path)), responses((status = 204)))]
pub async fn cancel(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<AttachmentId>,
) -> Result<StatusCode, AppError> {
    service::cancel_upload(&state, actor, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Authorize a short-lived immutable original or sanitized thumbnail download.
#[utoipa::path(get, path = "/api/v1/media/{id}/download", operation_id = "media_download", params(("id" = AttachmentId, Path), ("variant" = Option<DownloadVariant>, Query)), responses((status = 200, body = DownloadGrant)))]
pub async fn download(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<AttachmentId>,
    Query(query): Query<DownloadQuery>,
) -> Result<Response, AppError> {
    Ok(no_store(Json(
        service::download(&state, actor, id, query.variant).await?,
    )))
}

fn no_store(value: impl IntoResponse) -> Response {
    let mut response = value.into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}
