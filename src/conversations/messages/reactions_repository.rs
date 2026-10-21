//! Single-choice reaction SQL and reader-specific aggregate projection.

use sqlx::{Postgres, Transaction};

use crate::conversations::model::{ReactionKind, ReactionSummary};
use crate::{
    error::AppError,
    types::{MessageId, Seq, UserId},
};

pub enum ChoiceChange {
    Changed,
    Unchanged,
}

/// The caller holds the conversation lock; only changed desired state advances a revision.
pub async fn set_choice(
    transaction: &mut Transaction<'_, Postgres>,
    message: MessageId,
    user: UserId,
    kind: Option<ReactionKind>,
) -> Result<ChoiceChange, sqlx::Error> {
    let affected = if let Some(kind) = kind {
        sqlx::query(
            r#"
            INSERT INTO message_reactions(message_id, user_id, kind)
            VALUES ($1, $2, $3)
            ON CONFLICT (message_id, user_id)
            DO UPDATE
            SET kind = EXCLUDED.kind, updated_at = clock_timestamp()
            WHERE message_reactions.kind IS DISTINCT FROM EXCLUDED.kind
            "#,
        )
        .bind(message)
        .bind(user)
        .bind(kind.as_str())
        .execute(&mut **transaction)
        .await?
        .rows_affected()
    } else {
        sqlx::query(
            r#"
            DELETE FROM message_reactions
            WHERE message_id = $1 AND user_id = $2
            "#,
        )
        .bind(message)
        .bind(user)
        .execute(&mut **transaction)
        .await?
        .rows_affected()
    };

    Ok(if affected == 0 {
        ChoiceChange::Unchanged
    } else {
        ChoiceChange::Changed
    })
}

pub async fn update_revision(
    transaction: &mut Transaction<'_, Postgres>,
    message: MessageId,
    revision: Seq,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE messages
        SET revision_seq = $2
        WHERE id = $1
        "#,
    )
    .bind(message)
    .bind(revision)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
struct Aggregate {
    kind: String,
    count: i64,
    mine: bool,
}

/// Counts include former members; only the current reader's own row becomes `mine`.
pub async fn summaries(
    transaction: &mut Transaction<'_, Postgres>,
    message: MessageId,
    reader: UserId,
) -> Result<Vec<ReactionSummary>, AppError> {
    let rows: Vec<Aggregate> = sqlx::query_as(
        r#"
        SELECT kind, count(*) AS count, bool_or(user_id = $2) AS mine
        FROM message_reactions
        WHERE message_id = $1
        GROUP BY kind
        ORDER BY array_position(ARRAY['like','love','laugh','surprise','sad','celebrate'], kind)
        "#,
    )
    .bind(message)
    .bind(reader)
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ReactionSummary {
                kind: ReactionKind::from_record(&row.kind)?,
                count: u32::try_from(row.count).map_err(|_| AppError::unavailable())?,
                mine: row.mine,
            })
        })
        .collect()
}
