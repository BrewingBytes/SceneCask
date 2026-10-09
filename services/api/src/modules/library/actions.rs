//! Shared mutation action recorder (C03). Library writes record here, and the tracking mutation
//! service (R16) records episode changes through the same interface so one action can span both.
//! Call inside the transaction that performs the domain write, so the action and its changes
//! commit or roll back with it.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

/// C05: an action can be undone for 10 minutes.
pub const UNDO_WINDOW: Duration = Duration::minutes(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    EpisodeProgress,
    LibraryEntry,
}

impl Entity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EpisodeProgress => "episode_progress",
            Self::LibraryEntry => "library_entry",
        }
    }
}

/// One field written by an action: its value before the action and the entity revision the
/// action produced. Undo reverts the field only while that revision is still current.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub entity: Entity,
    /// The entity's ID within the action's user and show (show ID or episode ID).
    pub key: Uuid,
    pub field: &'static str,
    pub before: Value,
    pub after_revision: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: Uuid,
    pub undo_until: DateTime<Utc>,
}

impl Action {
    pub fn undo_until_rfc3339(&self) -> String {
        self.undo_until.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

/// Records an action of `kind` (lowercase snake case, for example `library_save`) owning
/// `changes`. Returns `None` without writing when nothing changed: a no-op has no action.
pub async fn record(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    kind: &str,
    changes: &[Change],
) -> Result<Option<Action>, sqlx::Error> {
    if changes.is_empty() {
        return Ok(None);
    }
    let (id, undo_until): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO mutation_actions (user_id, show_id, kind, undo_until)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4)) RETURNING id, undo_until",
    )
    .bind(user_id)
    .bind(show_id)
    .bind(kind)
    .bind(UNDO_WINDOW.num_seconds() as f64)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO mutation_changes (action_id, entity, entity_key, field, before_value, after_revision)
         SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::text[], $5::jsonb[], $6::bigint[])",
    )
    .bind(id)
    .bind(changes.iter().map(|c| c.entity.as_str()).collect::<Vec<_>>())
    .bind(changes.iter().map(|c| c.key.to_string()).collect::<Vec<_>>())
    .bind(changes.iter().map(|c| c.field).collect::<Vec<_>>())
    .bind(changes.iter().map(|c| sqlx::types::Json(&c.before)).collect::<Vec<_>>())
    .bind(changes.iter().map(|c| c.after_revision).collect::<Vec<_>>())
    .execute(&mut *conn)
    .await?;
    Ok(Some(Action { id, undo_until }))
}
