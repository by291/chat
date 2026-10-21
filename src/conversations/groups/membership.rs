//! Group creation, roles and membership episodes under one aggregate lock.

use sqlx::{Postgres, Transaction};

use crate::conversations::{
    events,
    groups::repository as persistence,
    model::{
        ConversationSummary, GroupConversationView, GroupMemberView, GroupRole, MembershipChange,
    },
    repository,
    service::{self, Member},
};
use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, MembershipId, Seq,
        UserId,
    },
};

fn title(input: &str) -> Result<&str, AppError> {
    let title = input.trim();
    if !(1..=100).contains(&title.chars().count()) {
        return Err(AppError::invalid("群名称需要 1–100 个字符"));
    }

    Ok(title)
}

/// Acquire the aggregate before member/pair locks, then fence the caller's episode.
pub(super) async fn lock_group(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
) -> Result<Member, AppError> {
    repository::lock_version(transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let member = service::require_membership(transaction, actor, id, Some(expected)).await?;
    if member.kind != "group" {
        return Err(AppError::not_found());
    }

    Ok(member)
}

pub(super) fn require_manager(member: &Member, owner_only: bool) -> Result<(), AppError> {
    if member.role != "owner" && (owner_only || member.role != "admin") {
        return Err(AppError::forbidden());
    }

    Ok(())
}

fn change(member: &Member, role: GroupRole, active: bool) -> MembershipChange {
    MembershipChange {
        user_id: member.user_id,
        membership_id: member.membership_id,
        role,
        joined_seq: Seq(member.joined_seq),
        active,
    }
}

pub(super) async fn summary(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
) -> Result<GroupConversationView, AppError> {
    match service::get_conversation(state, actor, id).await? {
        ConversationSummary::Group(group) => Ok(group),
        ConversationSummary::Direct(_) => Err(AppError::not_found()),
    }
}

/// Create a group; all initial memberships, the first event and outbox commit together.
pub async fn create(
    state: &AppState,
    actor: Actor,
    input: &str,
    member_ids: &[UserId],
) -> Result<GroupConversationView, AppError> {
    let title = title(input)?;
    let mut members = member_ids.to_vec();
    members.retain(|member| *member != actor.user_id);
    members.sort_unstable();
    members.dedup();
    if members.len() + 1 > usize::from(state.config.group_max_members) {
        return Err(AppError::conflict("群成员人数已满"));
    }
    let id = ConversationId::new();

    let mut transaction = state.pool.begin().await?;
    if !persistence::initial_users_exist(&mut transaction, &members).await? {
        return Err(AppError::invalid("所选用户不存在，请重新选择"));
    }

    for peer in &members {
        crate::friends::service::require_friend(&mut transaction, actor.user_id, *peer).await?;
    }
    persistence::insert_group(&mut transaction, id, title).await?;
    persistence::insert_member(
        &mut transaction,
        id,
        actor.user_id,
        MembershipId::new(),
        GroupRole::Owner,
        Seq(1),
    )
    .await?;
    // All initial members start at the creation event, so its fanout discovers
    // the complete group only after the same transaction commits.
    for user_id in members {
        persistence::insert_member(
            &mut transaction,
            id,
            user_id,
            MembershipId::new(),
            GroupRole::Member,
            Seq(1),
        )
        .await?;
    }
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::ConversationCreated,
        ConversationEventPayload::ConversationCreated {
            initiator_id: actor.user_id,
        },
    )
    .await?;

    transaction.commit().await?;

    summary(state, actor, id).await
}

/// Rename under the aggregate lock; ordinary members cannot mutate group metadata.
pub async fn rename(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    input: &str,
) -> Result<GroupConversationView, AppError> {
    let title = title(input)?;

    let mut transaction = state.pool.begin().await?;
    let member = lock_group(&mut transaction, actor, id, expected).await?;
    require_manager(&member, false)?;

    persistence::rename(&mut transaction, id, title).await?;
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::GroupUpdated,
        ConversationEventPayload::GroupUpdated {
            title: title.to_owned(),
        },
    )
    .await?;

    transaction.commit().await?;

    summary(state, actor, id).await
}

/// Read only the current roster; historical episodes never reappear as active members.
pub async fn members(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
) -> Result<Vec<GroupMemberView>, AppError> {
    let mut transaction = state.pool.begin().await?;
    let member = service::require_member(&mut transaction, actor, id).await?;
    if member.kind != "group" {
        return Err(AppError::not_found());
    }

    let roster = persistence::roster(&mut transaction, id).await?;

    transaction.commit().await?;

    Ok(roster)
}

/// Only the owner may change admin roles; ownership requires its separate atomic operation.
pub async fn set_role(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    user: UserId,
    role: GroupRole,
) -> Result<Vec<GroupMemberView>, AppError> {
    if role == GroupRole::Owner {
        return Err(AppError::invalid("请使用群主转让操作"));
    }

    let mut transaction = state.pool.begin().await?;
    let manager = lock_group(&mut transaction, actor, id, expected).await?;
    require_manager(&manager, true)?;

    let target = repository::member(&mut transaction, user, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    if target.role == "owner" {
        return Err(AppError::conflict("不能直接修改群主角色"));
    }

    persistence::set_role(&mut transaction, target.membership_id, role).await?;
    if role == GroupRole::Member {
        // Revocation is durable: later promotion must not revive old administrative links.
        persistence::revoke_issuer_invitations(&mut transaction, id, target.user_id).await?;
    }
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::MembershipUpdated,
        ConversationEventPayload::MembershipUpdated(vec![change(&target, role, true)]),
    )
    .await?;
    let roster = persistence::roster(&mut transaction, id).await?;

    transaction.commit().await?;

    Ok(roster)
}

/// Preserve one owner at commit; demote first to satisfy the unique active-owner index.
pub async fn transfer(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    user: UserId,
) -> Result<Vec<GroupMemberView>, AppError> {
    let mut transaction = state.pool.begin().await?;
    let owner = lock_group(&mut transaction, actor, id, expected).await?;
    require_manager(&owner, true)?;
    if user == actor.user_id {
        return Err(AppError::invalid("请选择另一位成员"));
    }

    let target = repository::member(&mut transaction, user, id)
        .await?
        .ok_or_else(AppError::not_found)?;

    persistence::set_role(&mut transaction, owner.membership_id, GroupRole::Admin).await?;
    persistence::set_role(&mut transaction, target.membership_id, GroupRole::Owner).await?;
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::MembershipUpdated,
        ConversationEventPayload::MembershipUpdated(vec![
            change(&owner, GroupRole::Admin, true),
            change(&target, GroupRole::Owner, true),
        ]),
    )
    .await?;
    let roster = persistence::roster(&mut transaction, id).await?;

    transaction.commit().await?;

    Ok(roster)
}

/// End an episode and invalidate its devices atomically. An owner must transfer first.
pub async fn remove(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    user: UserId,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    let member = lock_group(&mut transaction, actor, id, expected).await?;
    let target = repository::member(&mut transaction, user, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    if target.role == "owner" {
        return Err(AppError::conflict("群主需要先转让再退出"));
    }

    if user != actor.user_id {
        require_manager(&member, false)?;
        if target.role == "admin" {
            require_manager(&member, true)?;
        }
    }

    let envelope = events::append_with_invalidations(
        &mut transaction,
        id,
        ConversationEventKind::MembershipUpdated,
        ConversationEventPayload::MembershipUpdated(vec![change(
            &target,
            GroupRole::from_record(&target.role)?,
            false,
        )]),
        vec![crate::conversations::model::MembershipInvalidation {
            user_id: user,
            membership_id: target.membership_id,
        }],
    )
    .await?;
    persistence::revoke_issuer_invitations(&mut transaction, id, target.user_id).await?;
    persistence::end_membership(&mut transaction, target.membership_id, envelope.event_seq).await?;

    transaction.commit().await?;

    Ok(())
}

/// Add the actor's friends atomically under the group lock; ordinary members may invite.
/// Rejoining creates a fresh episode and never exposes earlier history.
pub async fn add_members(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    expected: MembershipId,
    member_ids: &[UserId],
) -> Result<Vec<GroupMemberView>, AppError> {
    if member_ids.len() > usize::from(state.config.group_max_members) {
        return Err(AppError::invalid("所选人数超过群成员上限"));
    }
    let mut candidates = member_ids.to_vec();
    candidates.sort_unstable();
    candidates.dedup();

    let mut transaction = state.pool.begin().await?;
    lock_group(&mut transaction, actor, id, expected).await?;

    // Sort candidates before pair locks; concurrent additions must acquire them in one order.
    let mut additions = Vec::new();
    for peer in candidates {
        if repository::member(&mut transaction, peer, id)
            .await?
            .is_none()
        {
            crate::friends::service::require_friend(&mut transaction, actor.user_id, peer).await?;
            additions.push(peer);
        }
    }

    if persistence::count_members(&mut transaction, id).await? + additions.len() as i64
        > i64::from(state.config.group_max_members)
    {
        return Err(AppError::conflict("群成员人数已满"));
    }

    if !additions.is_empty() {
        let latest = repository::lock_version(&mut transaction, id)
            .await?
            .ok_or_else(AppError::not_found)?;
        let joined_seq = Seq(latest.checked_add(1).ok_or_else(AppError::unavailable)?);
        let mut changes = Vec::new();
        for user_id in additions {
            let membership_id = MembershipId::new();
            persistence::insert_member(
                &mut transaction,
                id,
                user_id,
                membership_id,
                GroupRole::Member,
                joined_seq,
            )
            .await?;
            changes.push(MembershipChange {
                user_id,
                membership_id,
                role: GroupRole::Member,
                joined_seq,
                active: true,
            });
        }
        events::append(
            &mut transaction,
            id,
            ConversationEventKind::MembershipUpdated,
            ConversationEventPayload::MembershipUpdated(changes),
        )
        .await?;
    }

    transaction.commit().await?;

    members(state, actor, id).await
}
