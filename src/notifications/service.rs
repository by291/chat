//! Authenticated device capabilities and temporary activity; neither workflow changes read cursors.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use subtle::ConstantTimeEq;

use super::{
    model::{ActivityInput, PushConfigView, SubscriptionBinding, SubscriptionInput},
    repository,
};
use crate::{
    AppState,
    error::AppError,
    infrastructure::web_push::WebPushConfig,
    types::{Actor, PushSubscriptionId},
};

/// Expose only the public VAPID key; authentication remains required even in disabled mode.
pub fn config_view(state: &AppState) -> PushConfigView {
    PushConfigView {
        enabled: state.config.web_push.is_some(),
        vapid_public_key: state
            .config
            .web_push
            .as_ref()
            .map(|config| config.public_key.clone()),
    }
}

/// Register the current session's endpoint capability. Replacement creates a fresh binding generation.
/// Known foreign endpoints require their exact key capability before any row can be disabled.
pub async fn register_subscription(
    state: &AppState,
    actor: Actor,
    mut input: SubscriptionInput,
) -> Result<SubscriptionBinding, AppError> {
    let config = state.config.web_push.as_ref().ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "push_unavailable",
            "后台提醒暂未启用",
        )
    })?;
    let endpoint = validate_subscription(config, &input)?;
    input.endpoint = endpoint.to_string();
    let endpoint_hash = hex::encode(Sha256::digest(input.endpoint.as_bytes()));

    let mut transaction = state.pool.begin().await?;
    lock_actor(&mut transaction, actor).await?;
    repository::lock_endpoint(&mut transaction, &endpoint_hash).await?;
    let bindings =
        repository::lock_registration_bindings(&mut transaction, actor, &endpoint_hash).await?;
    if let Some(target) = bindings
        .iter()
        .find(|binding| binding.endpoint_hash == endpoint_hash)
    {
        let matches = target.p256dh == input.p256dh
            && bool::from(target.auth.as_bytes().ct_eq(input.auth.as_bytes()));
        let current_owner =
            target.user_id == actor.user_id && target.session_id == actor.session_id;
        if !current_owner && !matches {
            return Err(AppError::conflict("订阅凭据不匹配，请在当前设备重新启用"));
        }
        if current_owner && matches && target.revoked_at.is_none() {
            let result = SubscriptionBinding {
                id: target.id,
                generation: target.generation,
            };
            transaction.commit().await?;
            return Ok(result);
        }
    }
    for binding in &bindings {
        if binding.revoked_at.is_none() {
            repository::revoke_binding(&mut transaction, binding.id).await?;
        }
    }
    if repository::active_device_count(&mut transaction, actor.user_id).await? >= 10 {
        return Err(AppError::conflict("最多可为十个登录设备开启后台提醒"));
    }
    let binding =
        repository::insert_binding(&mut transaction, actor, &input, &endpoint_hash).await?;
    transaction.commit().await?;

    Ok(SubscriptionBinding {
        id: binding.id,
        generation: binding.generation,
    })
}

/// Remove only this actor's current-session binding; repeating the owned removal remains successful.
pub async fn remove_subscription(
    state: &AppState,
    actor: Actor,
    id: PushSubscriptionId,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    lock_actor(&mut transaction, actor).await?;
    repository::lock_owned_binding(&mut transaction, actor, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    repository::revoke_binding(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok(())
}

/// Renew a server-clock hint after checking binding ownership and conversation membership.
/// Hide/blur clears the hint; activity never applies events or acknowledges rendered messages.
pub async fn record_activity(
    state: &AppState,
    actor: Actor,
    id: PushSubscriptionId,
    input: ActivityInput,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    lock_actor(&mut transaction, actor).await?;
    let binding = repository::lock_owned_binding(&mut transaction, actor, id)
        .await?
        .filter(|binding| binding.revoked_at.is_none())
        .ok_or_else(AppError::not_found)?;
    if let Some(conversation) = input.conversation_id {
        crate::conversations::service::require_member(&mut transaction, actor, conversation)
            .await?;
    }
    repository::update_activity(
        &mut transaction,
        binding.id,
        input.conversation_id,
        input.visible && input.focused,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn lock_actor(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
) -> Result<(), AppError> {
    repository::lock_owner(transaction, actor.user_id).await?;
    if !crate::auth::security_repository::require_active_session(transaction, actor).await? {
        return Err(AppError::unauthorized());
    }
    Ok(())
}

fn validate_subscription(
    config: &WebPushConfig,
    input: &SubscriptionInput,
) -> Result<reqwest::Url, AppError> {
    if input.endpoint.len() > 2048 || input.p256dh.len() != 87 || input.auth.len() != 22 {
        return Err(AppError::invalid("推送订阅格式无效"));
    }
    let endpoint =
        reqwest::Url::parse(&input.endpoint).map_err(|_| AppError::invalid("推送订阅格式无效"))?;
    if endpoint.scheme() != "https"
        || endpoint.port().is_some_and(|port| port != 443)
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || !endpoint
            .host_str()
            .is_some_and(|host| config.allowed_hosts.iter().any(|allowed| allowed == host))
    {
        return Err(AppError::invalid("推送服务地址无效"));
    }
    let public = URL_SAFE_NO_PAD
        .decode(&input.p256dh)
        .map_err(|_| AppError::invalid("推送订阅格式无效"))?;
    let auth = URL_SAFE_NO_PAD
        .decode(&input.auth)
        .map_err(|_| AppError::invalid("推送订阅格式无效"))?;
    if public.len() != 65 || public.first() != Some(&4) || auth.len() != 16 {
        return Err(AppError::invalid("推送订阅格式无效"));
    }
    web_push_native::p256::PublicKey::from_sec1_bytes(&public)
        .map_err(|_| AppError::invalid("推送订阅格式无效"))?;
    Ok(endpoint)
}
