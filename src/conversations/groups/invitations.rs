//! Hashed single-use invitations with aggregate-first locking and atomic capacity checks.

use rand::RngCore;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::membership::{lock_group, require_manager, summary};
use super::repository as persistence;
use crate::{
    AppState,
    conversations::{
        events,
        model::{GroupConversationView, GroupInvitationView, GroupRole, MembershipChange},
        repository,
    },
    error::AppError,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, MembershipId, Seq,
    },
};

const INVITATION_LIFETIME_DAYS: i64 = 7;

/// Issue a hashed single-use token; only the creation response contains plaintext.
pub async fn invite(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
) -> Result<GroupInvitationView, AppError> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
    let invitation_id = Uuid::now_v7();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(INVITATION_LIFETIME_DAYS);

    let mut transaction = state.pool.begin().await?;
    let member = lock_group(&mut transaction, actor, id, expected).await?;
    require_manager(&member, false)?;

    persistence::insert_invitation(
        &mut transaction,
        invitation_id,
        id,
        actor.user_id,
        &token_hash,
        expires_at,
    )
    .await?;
    transaction.commit().await?;

    Ok(GroupInvitationView {
        invitation_id,
        token,
        expires_at,
    })
}

/// Revoke only group-scoped tokens under the same role and aggregate checks as creation.
pub async fn revoke_invite(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    invitation: Uuid,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    let member = lock_group(&mut transaction, actor, id, expected).await?;
    require_manager(&member, false)?;
    if !persistence::revoke_invitation(&mut transaction, id, invitation).await? {
        return Err(AppError::not_found());
    }
    transaction.commit().await?;

    Ok(())
}

/// Consume an invitation and capacity slot together; failed acceptance leaves both intact.
pub async fn accept(
    state: &AppState,
    actor: Actor,
    token: &str,
) -> Result<GroupConversationView, AppError> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::invalid("群邀请无效"));
    }

    let hash = hex::encode(Sha256::digest(token.as_bytes()));

    let mut transaction = state.pool.begin().await?;
    let id = persistence::invitation_conversation(&mut transaction, &hash)
        .await?
        .ok_or_else(AppError::not_found)?;

    // Preliminary lookup identifies the aggregate only. Lock it before the invitation,
    // then recheck every invitation condition to avoid inversion with group mutations.
    let latest = repository::lock_version(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let invitation = persistence::lock_invitation(&mut transaction, &hash)
        .await?
        .ok_or_else(AppError::not_found)?;
    if invitation.conversation_id != id
        || invitation.revoked_at.is_some()
        || invitation.consumed_at.is_some()
        || invitation.expires_at <= OffsetDateTime::now_utc()
    {
        return Err(AppError::conflict("群邀请已使用或失效"));
    }

    let issuer = repository::member(&mut transaction, invitation.issuer_id, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    require_manager(&issuer, false)?;
    if repository::member(&mut transaction, actor.user_id, id)
        .await?
        .is_some()
    {
        return Err(AppError::conflict("已经是群成员"));
    }
    if persistence::count_members(&mut transaction, id).await?
        >= i64::from(state.config.group_max_members)
    {
        return Err(AppError::conflict("群成员人数已满"));
    }

    let membership_id = MembershipId::new();
    let joined_seq = Seq(latest.checked_add(1).ok_or_else(AppError::unavailable)?);
    persistence::insert_member(
        &mut transaction,
        id,
        actor.user_id,
        membership_id,
        GroupRole::Member,
        joined_seq,
    )
    .await?;
    persistence::consume_invitation(&mut transaction, invitation.id, actor.user_id).await?;
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::MembershipUpdated,
        ConversationEventPayload::MembershipUpdated(vec![MembershipChange {
            user_id: actor.user_id,
            membership_id,
            role: GroupRole::Member,
            joined_seq,
            active: true,
        }]),
    )
    .await?;
    transaction.commit().await?;

    summary(state, actor, id).await
}
