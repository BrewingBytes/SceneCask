//! Library writes (C05): save, remove and manual status changes with expected revisions,
//! idempotency and action provenance in one transaction. Removal clears only the saved flag, so
//! progress and the manual status survive and return when the show is saved again.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{
    actions::{self, Change, Entity},
    dto::{MutationResult, Status},
    idempotency::{self, Claimed, IdempotencyKey},
    repository,
};
use crate::error::{ApiError, ErrorCode};

/// A validated library write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// `PUT`: save (keeping the existing status unless one is given) or remove.
    Save { saved: bool, status: Option<Status> },
    /// `PATCH`: change the status of a saved show.
    SetStatus(Status),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    saved: bool,
    status: Status,
    saved_at: Option<DateTime<Utc>>,
}

/// How a show without a library row reads.
const ABSENT: Entry = Entry {
    saved: false,
    status: Status::PlanToWatch,
    saved_at: None,
};

pub struct Mutation {
    pub user_id: Uuid,
    pub show_id: Uuid,
    pub key: IdempotencyKey,
    pub request_hash: Vec<u8>,
    pub expected_revision: i64,
    pub write: Write,
}

/// Applies `mutation` and returns the MutationResult JSON (or the stored one for a replayed
/// key). Rejections roll back every write, including the idempotency record.
pub async fn apply(pool: &PgPool, mutation: Mutation) -> Result<Value, ApiError> {
    let mut tx = pool.begin().await?;
    let claim = match idempotency::claim(
        &mut tx,
        mutation.user_id,
        mutation.key,
        mutation.request_hash.clone(),
    )
    .await?
    {
        Claimed::Replay(response) => return Ok(response),
        Claimed::New(claim) => claim,
    };
    let result = write(&mut tx, &mutation).await?;
    let response = claim.store(&mut tx, &result).await?;
    tx.commit().await?;
    Ok(response)
}

async fn write(conn: &mut PgConnection, mutation: &Mutation) -> Result<MutationResult, ApiError> {
    let (user_id, show_id) = (mutation.user_id, mutation.show_id);
    // Follows the shared show → tracking state → library entry order, skipping tracking state:
    // library writes never change it. An unknown show is 404.
    let now: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT now() FROM shows WHERE id = $1 FOR KEY SHARE")
            .bind(show_id)
            .fetch_optional(&mut *conn)
            .await?;
    let now = now.ok_or_else(ApiError::not_found)?;
    let row: Option<(bool, String, Option<DateTime<Utc>>, i64)> = sqlx::query_as(
        "SELECT saved, status, saved_at, revision FROM library_entries
         WHERE user_id = $1 AND show_id = $2 FOR UPDATE",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (before, revision) = match &row {
        Some((saved, status, saved_at, revision)) => (
            Entry {
                saved: *saved,
                status: Status::parse(status).ok_or_else(|| ApiError::unavailable("status"))?,
                saved_at: *saved_at,
            },
            *revision,
        ),
        None => (ABSENT, 0),
    };
    if matches!(mutation.write, Write::SetStatus(_)) && !before.saved {
        return Err(ApiError::not_found());
    }
    if mutation.expected_revision != revision {
        return Err(ApiError::new(ErrorCode::RevisionConflict));
    }
    let after = match mutation.write {
        Write::Save {
            saved: true,
            status,
        } => Entry {
            saved: true,
            status: status.unwrap_or(before.status),
            saved_at: if before.saved {
                before.saved_at
            } else {
                Some(now)
            },
        },
        Write::Save { saved: false, .. } => Entry {
            saved: false,
            ..before
        },
        Write::SetStatus(status) => Entry { status, ..before },
    };

    let mut changes = Vec::new();
    let mut action = None;
    if after != before {
        let new_revision: Option<i64> = match row {
            Some(_) => sqlx::query_scalar(
                "UPDATE library_entries SET saved = $3, status = $4, saved_at = $5,
                     revision = revision + 1, updated_at = now()
                 WHERE user_id = $1 AND show_id = $2 RETURNING revision",
            ),
            // A concurrent first save makes this a conflict rather than a second insert.
            None => sqlx::query_scalar(
                "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
                 VALUES ($1, $2, $3, $4, $5, 1) ON CONFLICT DO NOTHING RETURNING revision",
            ),
        }
        .bind(user_id)
        .bind(show_id)
        .bind(after.saved)
        .bind(after.status.as_str())
        .bind(after.saved_at)
        .fetch_optional(&mut *conn)
        .await?;
        let new_revision =
            new_revision.ok_or_else(|| ApiError::new(ErrorCode::RevisionConflict))?;
        changes = field_changes(show_id, &before, &after, new_revision);
        let kind = match mutation.write {
            Write::Save { saved: true, .. } => "library_save",
            Write::Save { saved: false, .. } => "library_remove",
            Write::SetStatus(_) => "library_status",
        };
        action = actions::record(conn, user_id, show_id, kind, &changes).await?;
    }

    let library = repository::items(conn, user_id, &[show_id], now)
        .await?
        .pop()
        .ok_or_else(ApiError::not_found)?;
    let tracking_revision: i64 = sqlx::query_scalar(
        "SELECT coalesce((SELECT revision FROM tracking_show_state
                          WHERE user_id = $1 AND show_id = $2), 0)",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(MutationResult {
        action_id: action.map(|action| action.id),
        undo_until: action.map(|action| action.undo_until_rfc3339()),
        changed: u32::from(!changes.is_empty()),
        library,
        tracking_revision,
        episodes: Vec::new(),
    })
}

/// The fields the write changed, with their prior values for undo.
fn field_changes(show_id: Uuid, before: &Entry, after: &Entry, revision: i64) -> Vec<Change> {
    let saved_at = |at: Option<DateTime<Utc>>| {
        at.map_or(Value::Null, |at| {
            json!(at.to_rfc3339_opts(SecondsFormat::Micros, true))
        })
    };
    let fields = [
        ("saved", before.saved != after.saved, json!(before.saved)),
        (
            "status",
            before.status != after.status,
            json!(before.status.as_str()),
        ),
        (
            "saved_at",
            before.saved_at != after.saved_at,
            saved_at(before.saved_at),
        ),
    ];
    fields
        .into_iter()
        .filter(|(_, changed, _)| *changed)
        .map(|(field, _, before)| Change {
            entity: Entity::LibraryEntry,
            key: show_id,
            field,
            before,
            after_revision: revision,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn changes_record_only_changed_fields_with_prior_values() {
        let at = Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap();
        let show = Uuid::from_u128(7);
        let saved = Entry {
            saved: true,
            status: Status::Watching,
            saved_at: Some(at),
        };
        let changes = field_changes(show, &ABSENT, &saved, 1);
        let fields: Vec<_> = changes
            .iter()
            .map(|c| (c.field, c.before.clone()))
            .collect();
        assert_eq!(
            fields,
            [
                ("saved", json!(false)),
                ("status", json!("plan_to_watch")),
                ("saved_at", Value::Null),
            ]
        );
        assert!(
            changes.iter().all(|c| c.after_revision == 1
                && c.key == show
                && c.entity == Entity::LibraryEntry)
        );

        let removed = Entry {
            saved: false,
            ..saved
        };
        let changes = field_changes(show, &saved, &removed, 2);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            (changes[0].field, &changes[0].before),
            ("saved", &json!(true))
        );
        assert!(field_changes(show, &saved, &saved, 3).is_empty());
    }
}
