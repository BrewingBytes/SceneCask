//! Action Undo (C05) for every action the shared recorder stores: individual marks, history
//! erase and library writes. A change is reverted only while the revision it produced is still
//! current, so later edits survive even when they wrote the same value; reversals advance
//! revisions instead of restoring old ones. The outcome is stored on the action, so a repeated
//! Undo returns it unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, types::Json};
use uuid::Uuid;

use super::mutations::{bump_tracking, lock_tracking, write_progress};
use crate::{
    error::{ApiError, ErrorCode},
    modules::library::{
        actions::Entity,
        dto::LibraryItem,
        idempotency::{self, IdempotencyKey},
        service::{self as library, LockedEntry},
    },
};

/// `{actionId,reverted,skipped,library}`. Counts are per changed entity: each episode, and the
/// library entry once whatever number of its fields the action wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub action_id: Uuid,
    pub reverted: u32,
    pub skipped: u32,
    pub library: LibraryItem,
}

/// Undoes the caller's action `action_id`. Another user's or an unknown action is 404; an
/// action past its window that was never undone is 410 ACTION_EXPIRED.
pub async fn undo(
    pool: &PgPool,
    user_id: Uuid,
    key: IdempotencyKey,
    request_hash: Vec<u8>,
    action_id: Uuid,
) -> Result<Value, ApiError> {
    idempotency::transact(pool, user_id, key, request_hash, async |conn| {
        undo_in(conn, user_id, action_id).await
    })
    .await
}

async fn undo_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    action_id: Uuid,
) -> Result<Value, ApiError> {
    // The action row serializes undos of one action; it is locked before the shared order and
    // no other writer locks it.
    let action: Option<(Uuid, bool, Option<Json<Value>>)> = sqlx::query_as(
        "SELECT show_id, undo_until > now(), undo_result FROM mutation_actions
         WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(action_id)
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (show_id, open, stored) = action.ok_or_else(ApiError::not_found)?;
    if let Some(Json(stored)) = stored {
        return Ok(stored);
    }
    if !open {
        return Err(ApiError::new(ErrorCode::ActionExpired));
    }

    let rows: Vec<(String, String, String, Json<Value>, i64)> = sqlx::query_as(
        "SELECT entity, entity_key, field, before_value, after_revision FROM mutation_changes
         WHERE action_id = $1",
    )
    .bind(action_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut episodes: BTreeMap<Uuid, (bool, i64)> = BTreeMap::new();
    let mut entry_fields: Vec<(String, Value)> = Vec::new();
    let mut entry_revision = None;
    for (entity, key, field, Json(before), after_revision) in rows {
        let corrupt = || ApiError::unavailable("undo");
        match entity.as_str() {
            e if e == Entity::EpisodeProgress.as_str() && field == "watched" => {
                let id = Uuid::try_parse(&key).map_err(|_| corrupt())?;
                episodes.insert(id, (before.as_bool().ok_or_else(corrupt)?, after_revision));
            }
            e if e == Entity::LibraryEntry.as_str() && key == show_id.to_string() => {
                entry_fields.push((field, before));
                entry_revision = Some(after_revision);
            }
            _ => return Err(corrupt()),
        }
    }

    let now = library::lock_show(conn, show_id).await?;
    let tracking = match episodes.is_empty() {
        true => None,
        false => Some(lock_tracking(conn, user_id, show_id).await?),
    };
    let (mut reverted, mut skipped) = (0, 0);
    let mut unsaved = false;
    if let Some(after_revision) = entry_revision {
        let entry = LockedEntry::lock(conn, user_id, show_id).await?;
        if entry.revision == after_revision {
            let prior = entry_fields
                .iter()
                .try_fold(entry.before, |state, (field, value)| {
                    state.with_prior(field, value)
                })
                .ok_or_else(|| ApiError::unavailable("undo"))?;
            unsaved = entry.before.saved && !prior.saved;
            entry.store(conn, prior).await?;
            reverted += 1;
        } else {
            skipped += 1;
        }
    }

    let ids: Vec<Uuid> = episodes.keys().copied().collect();
    let current: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT episode_id, revision FROM episode_progress
         WHERE user_id = $1 AND episode_id = ANY($2) ORDER BY episode_id FOR UPDATE",
    )
    .bind(user_id)
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let writes: Vec<(Uuid, bool)> = current
        .into_iter()
        .filter_map(|(id, revision)| {
            let (before, after_revision) = episodes[&id];
            (revision == after_revision).then_some((id, before))
        })
        .collect();
    skipped += (episodes.len() - writes.len()) as u32;
    reverted += writes.len() as u32;
    if let (Some(locked), false) = (tracking, writes.is_empty()) {
        write_progress(conn, user_id, &writes, Some(action_id)).await?;
        bump_tracking(conn, user_id, show_id, locked).await?;
    }

    // Activity the action produced for fields now reversed: watched marks it set and the save
    // it made. Social activity projection writes these rows; undo only removes them.
    let unwatched: Vec<Uuid> = writes
        .iter()
        .filter(|&&(_, before)| !before)
        .map(|&(id, _)| id)
        .collect();
    sqlx::query(
        "DELETE FROM activity_events WHERE action_id = $1
           AND ((kind = 'episode_watched' AND episode_id = ANY($2))
                OR (kind = 'library_added' AND $3))",
    )
    .bind(action_id)
    .bind(&unwatched)
    .bind(unsaved)
    .execute(&mut *conn)
    .await?;

    let outcome = serde_json::to_value(UndoResult {
        action_id,
        reverted,
        skipped,
        library: library::item(conn, user_id, show_id, now).await?,
    })
    .map_err(|_| ApiError::unavailable("undo"))?;
    sqlx::query("UPDATE mutation_actions SET undone_at = now(), undo_result = $2 WHERE id = $1")
        .bind(action_id)
        .bind(Json(&outcome))
        .execute(&mut *conn)
        .await?;
    Ok(outcome)
}
