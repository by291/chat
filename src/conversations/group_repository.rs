//! Group membership and invitation persistence. The application owns locks and commits.

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::model::{GroupMemberView, GroupRole};
use crate::{
    error::AppError,
    types::{ConversationId, MembershipId, Seq, UserId, UserView},
};

#[derive(sqlx::FromRow)]
pub struct Invitation {
    pub id: Uuid,
    pub conversation_id: ConversationId,
    pub issuer_id: UserId,
    pub expires_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
    pub consumed_at: Option<OffsetDateTime>,
}

#[derive(sqlx::FromRow)]
struct RosterRow {
    membership_id: MembershipId,
    user_id: UserId,
    username: String,
    display_name: String,
    role: String,
    joined_seq: i64,
    last_read_message_seq: i64,
}

/// Lock users in ID order so initial membership cannot race a user deletion.
pub async fn initial_users_exist(
    tx: &mut Transaction<'_, Postgres>,
    members: &[UserId],
) -> Result<bool, sqlx::Error> {
    let users: Vec<UserId> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM users
        WHERE id = ANY($1)
        ORDER BY id
        FOR KEY SHARE
        "#,
    )
    .bind(members)
    .fetch_all(&mut **tx)
    .await?;

    Ok(users.len() == members.len())
}

pub async fn insert_group(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO conversations(id, kind, title)
        VALUES ($1, 'group', $2)
        "#,
    )
    .bind(id)
    .bind(title)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn insert_member(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    user: UserId,
    membership: MembershipId,
    role: GroupRole,
    joined_seq: Seq,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO conversation_members(
            conversation_id, user_id, membership_id, role, joined_seq, last_read_message_seq
        )
        VALUES ($1, $2, $3, $4, $5, 0)
        "#,
    )
    .bind(id)
    .bind(user)
    .bind(membership)
    .bind(role.as_str())
    .bind(joined_seq)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn count_members(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM conversation_members
        WHERE conversation_id = $1
            AND left_at IS NULL
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

pub async fn rename(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conversations
        SET title = $2
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(title)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn set_role(
    tx: &mut Transaction<'_, Postgres>,
    membership: MembershipId,
    role: GroupRole,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conversation_members
        SET role = $2
        WHERE membership_id = $1
            AND left_at IS NULL
        "#,
    )
    .bind(membership)
    .bind(role.as_str())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn end_membership(
    tx: &mut Transaction<'_, Postgres>,
    membership: MembershipId,
    left_seq: Seq,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conversation_members
        SET left_at = now(), left_seq = $2
        WHERE membership_id = $1
            AND left_at IS NULL
        "#,
    )
    .bind(membership)
    .bind(left_seq)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn roster(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<Vec<GroupMemberView>, AppError> {
    let rows: Vec<RosterRow> = sqlx::query_as(
        r#"
        SELECT m.membership_id, m.user_id, u.username, u.display_name,
            m.role, m.joined_seq, m.last_read_message_seq
        FROM conversation_members m
        JOIN users u ON u.id = m.user_id
        WHERE m.conversation_id = $1
            AND m.left_at IS NULL
        ORDER BY m.joined_seq, m.membership_id
        "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(GroupMemberView {
                membership_id: row.membership_id,
                user: UserView {
                    id: row.user_id,
                    username: row.username,
                    display_name: row.display_name,
                },
                role: GroupRole::from_record(&row.role)?,
                joined_seq: Seq(row.joined_seq),
                read_seq: Seq(row.last_read_message_seq),
            })
        })
        .collect()
}

pub async fn insert_invitation(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    conversation: ConversationId,
    issuer: UserId,
    hash: &str,
    expires: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO group_invitations(id, conversation_id, issuer_id, token_hash, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(conversation)
    .bind(issuer)
    .bind(hash)
    .bind(expires)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn invitation_conversation(
    tx: &mut Transaction<'_, Postgres>,
    hash: &str,
) -> Result<Option<ConversationId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT conversation_id
        FROM group_invitations
        WHERE token_hash = $1
        "#,
    )
    .bind(hash)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn lock_invitation(
    tx: &mut Transaction<'_, Postgres>,
    hash: &str,
) -> Result<Option<Invitation>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, issuer_id, expires_at, revoked_at, consumed_at
        FROM group_invitations
        WHERE token_hash = $1
        FOR UPDATE
        "#,
    )
    .bind(hash)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn consume_invitation(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    user: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE group_invitations
        SET consumed_at = now(), consumed_by = $2
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(user)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn revoke_invitation(
    tx: &mut Transaction<'_, Postgres>,
    conversation: ConversationId,
    invitation: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE group_invitations
        SET revoked_at = coalesce(revoked_at, now())
        WHERE id = $1
            AND conversation_id = $2
        "#,
    )
    .bind(invitation)
    .bind(conversation)
    .execute(&mut **tx)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn revoke_issuer_invitations(
    tx: &mut Transaction<'_, Postgres>,
    conversation: ConversationId,
    issuer: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE group_invitations
        SET revoked_at = now()
        WHERE conversation_id = $1
            AND issuer_id = $2
            AND revoked_at IS NULL
            AND consumed_at IS NULL
        "#,
    )
    .bind(conversation)
    .bind(issuer)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
