//! Authorization and session leases for the private realtime proxy.

use axum::http::StatusCode;
use time::OffsetDateTime;

use super::{
    model::{
        ConnectMeta, ConnectReply, ConnectResult, Disconnect, ProxyRefreshRequest, RefreshReply,
        RefreshResult,
    },
    repository,
};
use crate::{AppState, auth::service::authenticate, error::AppError};

pub(crate) use super::repository::OutboxStats;

pub(crate) async fn outbox_stats(state: &AppState) -> Result<OutboxStats, AppError> {
    Ok(repository::outbox_stats(&state.pool).await?)
}

fn denied() -> ConnectReply {
    metrics::counter!("chat_proxy_failures_total","proxy"=>"connect").increment(1);
    ConnectReply::Denied {
        disconnect: Disconnect {
            code: 4501,
            reason: "unauthorized",
        },
    }
}

/// Authenticate a browser session and attach a short-lived private connection lease.
pub async fn connect_proxy(
    state: &AppState,
    client: &str,
    origin: Option<&str>,
    session_token: Option<&str>,
) -> Result<ConnectReply, AppError> {
    if client.is_empty() || client.len() > 128 {
        return Ok(denied());
    }

    if !origin.is_some_and(|origin| {
        state
            .config
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    }) {
        tracing::warn!(reason = "untrusted_origin", "realtime connect denied");
        return Ok(denied());
    }

    let Some(token) = session_token else {
        tracing::warn!(reason = "missing_cookie", "realtime connect denied");
        return Ok(denied());
    };

    let actor = match authenticate(state, token).await {
        Ok(actor) => actor,
        Err(error) if error.status == StatusCode::UNAUTHORIZED => {
            tracing::warn!(reason = "invalid_session", "realtime connect denied");
            return Ok(denied());
        }
        Err(error) => return Err(error),
    };

    let expires = OffsetDateTime::now_utc() + time::Duration::seconds(60);
    let saved =
        repository::attach_connection(&state.pool, client, actor.session_id, expires).await?;
    if saved != 1 {
        return Ok(denied());
    }

    Ok(ConnectReply::Accepted {
        result: ConnectResult {
            user: actor.user_id,
            expire_at: expires.unix_timestamp(),
            meta: ConnectMeta {
                session_id: actor.session_id,
            },
        },
    })
}

/// Renew only a lease that still belongs to the supplied live session and user.
pub async fn refresh_proxy(
    state: &AppState,
    input: ProxyRefreshRequest,
) -> Result<RefreshReply, AppError> {
    let expired = || {
        metrics::counter!("chat_proxy_failures_total","proxy"=>"refresh").increment(1);
        RefreshReply {
            result: RefreshResult {
                expired: Some(true),
                expire_at: None,
            },
        }
    };
    let Some(meta) = input.meta else {
        return Ok(expired());
    };

    let expires = OffsetDateTime::now_utc() + time::Duration::seconds(60);
    let saved = repository::refresh_connection(
        &state.pool,
        &input.client,
        meta.session_id,
        input.user,
        expires,
    )
    .await?;
    if saved != 1 {
        return Ok(expired());
    }

    Ok(RefreshReply {
        result: RefreshResult {
            expired: None,
            expire_at: Some(expires.unix_timestamp()),
        },
    })
}
