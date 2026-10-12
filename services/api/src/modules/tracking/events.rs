//! R27 activity projection (C06). Effective library saves and watched marks write
//! `activity_events` rows inside the transaction that records their action, so a rejected or
//! no-op write projects nothing and an idempotent replay never reaches this code. An event holds
//! only IDs: no episode title, overview or still is copied. Undo deletes the events of the fields
//! it reverses ([`super::undo`]); readers recheck follows, privacy and blocks at read time.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::modules::library::actions::{Action, Change, Entity};

/// `library_added` when `changes` saved a show that was not saved before. Saves made by a
/// watched mark project only the mark, so one mark or catch-up yields at most one event.
pub async fn library_added(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    action: Option<Action>,
    changes: &[Change],
) -> Result<(), sqlx::Error> {
    let added = changes.iter().any(|change| {
        change.entity == Entity::LibraryEntry
            && change.field == "saved"
            && change.before == Value::Bool(false)
    });
    match (action, added) {
        (Some(action), true) => record(conn, user_id, show_id, None, action).await,
        _ => Ok(()),
    }
}

/// `episode_watched` for `episode_id` when `changes` set it from unwatched. Catch-up passes only
/// its endpoint, so a bulk mark never fans out into one event per episode.
pub async fn episode_watched(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    episode_id: Uuid,
    action: Option<Action>,
    changes: &[Change],
) -> Result<(), sqlx::Error> {
    let marked = changes.iter().any(|change| {
        change.entity == Entity::EpisodeProgress
            && change.key == episode_id
            && change.before == Value::Bool(false)
    });
    match (action, marked) {
        (Some(action), true) => record(conn, user_id, show_id, Some(episode_id), action).await,
        _ => Ok(()),
    }
}

/// Inserts the event, replacing an earlier one for the same save or episode: reaching it again
/// needed a removal or unmark in between, so the feed keeps only the latest. The caller holds
/// the show's tracking or library locks, which serialize this per user and show.
async fn record(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    episode_id: Option<Uuid>,
    action: Action,
) -> Result<(), sqlx::Error> {
    let kind = match episode_id {
        Some(_) => "episode_watched",
        None => "library_added",
    };
    sqlx::query(
        "DELETE FROM activity_events
         WHERE actor_id = $1 AND show_id = $2 AND kind = $3
           AND episode_id IS NOT DISTINCT FROM $4",
    )
    .bind(user_id)
    .bind(show_id)
    .bind(kind)
    .bind(episode_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO activity_events (actor_id, kind, show_id, episode_id, action_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(show_id)
    .bind(episode_id)
    .bind(action.id)
    .execute(conn)
    .await
    .map(drop)
}
