//! Individual watched marks and history erase (C05). Each write takes the shared show → tracking
//! state → library entry → episode lock order, checks its expected revision, records one action
//! through the library action recorder and stores its idempotent response in the same
//! transaction. Writes never touch catalog rows, so a marked future episode stays future.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    error::{ApiError, ErrorCode},
    modules::library::{
        actions::{self, Action, Change, Entity},
        dto::{EpisodeChange, MutationResult},
        idempotency::{self, IdempotencyKey},
        service::{self as library, LockedEntry},
    },
};

/// `PUT /progress/episodes/{episodeId}`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgressRequest {
    pub watched: bool,
    pub expected_revision: i64,
}

/// `DELETE /shows/{id}/history`; `expectedRevision` is the aggregate tracking revision.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    pub expected_revision: i64,
}

/// The idempotency key, request fingerprint and validated expected revision of a write.
pub struct Keyed {
    pub user_id: Uuid,
    pub key: IdempotencyKey,
    pub request_hash: Vec<u8>,
    pub expected_revision: i64,
}

/// Marks one episode watched or unwatched. Released, future, undated and special episodes are
/// all accepted individually; archived episodes are not part of the catalog and read as 404.
pub async fn set_episode(
    pool: &PgPool,
    keyed: Keyed,
    episode_id: Uuid,
    watched: bool,
) -> Result<serde_json::Value, ApiError> {
    idempotency::transact(
        pool,
        keyed.user_id,
        keyed.key,
        keyed.request_hash,
        async |conn| {
            set_episode_in(
                conn,
                keyed.user_id,
                keyed.expected_revision,
                episode_id,
                watched,
            )
            .await
        },
    )
    .await
}

async fn set_episode_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    expected_revision: i64,
    episode_id: Uuid,
    watched: bool,
) -> Result<MutationResult, ApiError> {
    let show_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT e.show_id FROM episodes e JOIN seasons se ON se.id = e.season_id
         WHERE e.id = $1 AND e.archived_at IS NULL AND se.archived_at IS NULL",
    )
    .bind(episode_id)
    .fetch_optional(&mut *conn)
    .await?;
    let show_id = show_id.ok_or_else(ApiError::not_found)?;
    let now = library::lock_show(conn, show_id).await?;
    let tracking_revision = lock_tracking(conn, user_id, show_id).await?;
    // Only a true mark can change the library entry; false never adds the show.
    let entry = match watched {
        true => Some(LockedEntry::lock(conn, user_id, show_id).await?),
        false => None,
    };
    let current: Option<(bool, i64)> = sqlx::query_as(
        "SELECT watched, revision FROM episode_progress
         WHERE user_id = $1 AND episode_id = $2 FOR UPDATE",
    )
    .bind(user_id)
    .bind(episode_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (was_watched, revision) = current.unwrap_or((false, 0));
    if expected_revision != revision {
        return Err(ApiError::new(ErrorCode::RevisionConflict));
    }
    if was_watched == watched {
        let unchanged = EpisodeChange {
            id: episode_id,
            watched,
            revision,
        };
        return result(conn, user_id, show_id, now, None, vec![unchanged], 0).await;
    }

    let mut changes = match entry {
        Some(entry) => auto_add(conn, user_id, show_id, entry, now).await?,
        None => Vec::new(),
    };
    changes.push(Change {
        entity: Entity::EpisodeProgress,
        key: episode_id,
        field: "watched",
        before: json!(was_watched),
        after_revision: revision + 1,
    });
    let kind = if watched {
        "episode_watched"
    } else {
        "episode_unwatched"
    };
    let action = actions::record(conn, user_id, show_id, kind, &changes).await?;
    let episodes = write_progress(
        conn,
        user_id,
        &[(episode_id, watched)],
        action.map(|a| a.id),
    )
    .await?;
    bump_tracking(conn, user_id, show_id, tracking_revision).await?;
    result(conn, user_id, show_id, now, action, episodes, 1).await
}

/// Saves the show for a true mark and upgrades Plan to watch. An absent entry cannot be locked, so
/// a concurrent first library save can insert it first; the mark then locks and upgrades that
/// row instead of failing, because its expected revision names the episode, not the entry.
async fn auto_add(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    entry: LockedEntry,
    now: DateTime<Utc>,
) -> Result<Vec<Change>, ApiError> {
    match entry.store(conn, entry.before.watched(now)).await {
        Err(error) if error.code() == ErrorCode::RevisionConflict && entry.revision == 0 => {
            let entry = LockedEntry::lock(conn, user_id, show_id).await?;
            entry.store(conn, entry.before.watched(now)).await
        }
        stored => stored,
    }
}

/// Sets every watched episode of the show false, specials and archived episodes included, and
/// keeps the saved flag and manual status. Nothing watched is a no-op.
pub async fn erase_history(
    pool: &PgPool,
    keyed: Keyed,
    show_id: Uuid,
) -> Result<serde_json::Value, ApiError> {
    idempotency::transact(
        pool,
        keyed.user_id,
        keyed.key,
        keyed.request_hash,
        async |conn| erase_history_in(conn, keyed.user_id, keyed.expected_revision, show_id).await,
    )
    .await
}

async fn erase_history_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    expected_revision: i64,
    show_id: Uuid,
) -> Result<MutationResult, ApiError> {
    let now = library::lock_show(conn, show_id).await?;
    let tracking_revision = lock_tracking(conn, user_id, show_id).await?;
    if expected_revision != tracking_revision {
        return Err(ApiError::new(ErrorCode::RevisionConflict));
    }
    let watched: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT p.episode_id, p.revision FROM episode_progress p
         JOIN episodes e ON e.id = p.episode_id
         WHERE p.user_id = $1 AND e.show_id = $2 AND p.watched
         ORDER BY p.episode_id FOR UPDATE OF p",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_all(&mut *conn)
    .await?;
    if watched.is_empty() {
        return result(conn, user_id, show_id, now, None, Vec::new(), 0).await;
    }
    let changes: Vec<Change> = watched
        .iter()
        .map(|&(episode_id, revision)| Change {
            entity: Entity::EpisodeProgress,
            key: episode_id,
            field: "watched",
            before: json!(true),
            after_revision: revision + 1,
        })
        .collect();
    let action = actions::record(conn, user_id, show_id, "history_erase", &changes).await?;
    let writes: Vec<(Uuid, bool)> = watched.iter().map(|&(id, _)| (id, false)).collect();
    let episodes = write_progress(conn, user_id, &writes, action.map(|a| a.id)).await?;
    bump_tracking(conn, user_id, show_id, tracking_revision).await?;
    let changed = episodes.len() as u32;
    result(conn, user_id, show_id, now, action, episodes, changed).await
}

/// Creates (at revision 0) or locks the aggregate tracking row that serializes progress writes
/// for this user and show, and returns its revision.
pub(super) async fn lock_tracking(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar(
        "INSERT INTO tracking_show_state (user_id, show_id) VALUES ($1, $2)
         ON CONFLICT (user_id, show_id) DO UPDATE SET revision = tracking_show_state.revision
         RETURNING revision",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_one(conn)
    .await?)
}

/// Advances the tracking revision locked by [`lock_tracking`] once for an effective write or
/// reversal.
pub(super) async fn bump_tracking(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    locked: i64,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE tracking_show_state SET revision = $3 + 1 WHERE user_id = $1 AND show_id = $2",
    )
    .bind(user_id)
    .bind(show_id)
    .bind(locked)
    .execute(conn)
    .await?;
    Ok(())
}

/// Writes locked progress rows at their next revision, creating absent ones at revision 1, and
/// links them to `action_id`. False rows are kept for revision provenance.
pub(super) async fn write_progress(
    conn: &mut PgConnection,
    user_id: Uuid,
    writes: &[(Uuid, bool)],
    action_id: Option<Uuid>,
) -> Result<Vec<EpisodeChange>, ApiError> {
    let rows: Vec<(Uuid, bool, i64)> = sqlx::query_as(
        "INSERT INTO episode_progress (user_id, episode_id, watched, revision, last_action_id)
         SELECT $1, w.episode_id, w.watched, 1, $4
         FROM UNNEST($2::uuid[], $3::boolean[]) AS w (episode_id, watched)
         ON CONFLICT (user_id, episode_id) DO UPDATE SET watched = EXCLUDED.watched,
             revision = episode_progress.revision + 1,
             last_action_id = EXCLUDED.last_action_id, updated_at = now()
         RETURNING episode_id, watched, revision",
    )
    .bind(user_id)
    .bind(writes.iter().map(|&(id, _)| id).collect::<Vec<_>>())
    .bind(
        writes
            .iter()
            .map(|&(_, watched)| watched)
            .collect::<Vec<_>>(),
    )
    .bind(action_id)
    .fetch_all(conn)
    .await?;
    let mut episodes: Vec<EpisodeChange> = rows
        .into_iter()
        .map(|(id, watched, revision)| EpisodeChange {
            id,
            watched,
            revision,
        })
        .collect();
    episodes.sort_by_key(|episode| episode.id);
    Ok(episodes)
}

pub(super) async fn result(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    now: DateTime<Utc>,
    action: Option<Action>,
    episodes: Vec<EpisodeChange>,
    changed: u32,
) -> Result<MutationResult, ApiError> {
    Ok(MutationResult {
        action_id: action.map(|action| action.id),
        undo_until: action.map(|action| action.undo_until_rfc3339()),
        changed,
        library: library::item(conn, user_id, show_id, now).await?,
        tracking_revision: library::tracking_revision(conn, user_id, show_id).await?,
        episodes,
    })
}
