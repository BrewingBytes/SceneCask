//! Library writes (C05): save, remove and manual status changes with expected revisions,
//! idempotency and action provenance in one transaction. Removal clears only the saved flag, so
//! progress and the manual status survive and return when the show is saved again.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{
    actions::{self, Change, Entity},
    dto::{LibraryItem, MutationResult, Status},
    idempotency::{self, IdempotencyKey},
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
    idempotency::transact(
        pool,
        mutation.user_id,
        mutation.key,
        mutation.request_hash.clone(),
        async |conn| write(conn, &mutation).await,
    )
    .await
}

async fn write(conn: &mut PgConnection, mutation: &Mutation) -> Result<MutationResult, ApiError> {
    let (user_id, show_id) = (mutation.user_id, mutation.show_id);
    // Follows the shared show → tracking state → library entry order, skipping tracking state:
    // library writes never change it. An unknown show is 404.
    let now = lock_show(conn, show_id).await?;
    let entry = LockedEntry::lock(conn, user_id, show_id).await?;
    let before = entry.before;
    if matches!(mutation.write, Write::SetStatus(_)) && !before.saved {
        return Err(ApiError::not_found());
    }
    if mutation.expected_revision != entry.revision {
        return Err(ApiError::new(ErrorCode::RevisionConflict));
    }
    let after = match mutation.write {
        Write::Save {
            saved: true,
            status,
        } => Entry {
            status: status.unwrap_or(before.status),
            ..before.saved(now)
        },
        Write::Save { saved: false, .. } => Entry {
            saved: false,
            ..before
        },
        Write::SetStatus(status) => Entry { status, ..before },
    };

    let changes = entry.store(conn, after).await?;
    let kind = match mutation.write {
        Write::Save { saved: true, .. } => "library_save",
        Write::Save { saved: false, .. } => "library_remove",
        Write::SetStatus(_) => "library_status",
    };
    let action = actions::record(conn, user_id, show_id, kind, &changes).await?;
    Ok(MutationResult {
        action_id: action.map(|action| action.id),
        undo_until: action.map(|action| action.undo_until_rfc3339()),
        changed: u32::from(!changes.is_empty()),
        library: item(conn, user_id, show_id, now).await?,
        tracking_revision: tracking_revision(conn, user_id, show_id).await?,
        episodes: Vec::new(),
    })
}

/// Takes the first lock of the shared show → tracking state → library entry → episode order and
/// returns the transaction time. An unknown show is 404.
pub async fn lock_show(conn: &mut PgConnection, show_id: Uuid) -> Result<DateTime<Utc>, ApiError> {
    let now: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT now() FROM shows WHERE id = $1 FOR KEY SHARE")
            .bind(show_id)
            .fetch_optional(&mut *conn)
            .await?;
    now.ok_or_else(ApiError::not_found)
}

/// The viewer's LibraryItem for a show locked with [`lock_show`].
pub async fn item(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    now: DateTime<Utc>,
) -> Result<LibraryItem, ApiError> {
    repository::items(conn, user_id, &[show_id], now)
        .await?
        .pop()
        .ok_or_else(ApiError::not_found)
}

/// The aggregate tracking revision; 0 before the first episode write.
pub async fn tracking_revision(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT revision FROM tracking_show_state
                          WHERE user_id = $1 AND show_id = $2), 0)",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_one(conn)
    .await?)
}

/// A library entry's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub saved: bool,
    pub status: Status,
    pub saved_at: Option<DateTime<Utc>>,
}

impl Entry {
    /// Saved, keeping the original save time of an already saved entry.
    fn saved(self, now: DateTime<Utc>) -> Self {
        Self {
            saved: true,
            saved_at: if self.saved { self.saved_at } else { Some(now) },
            ..self
        }
    }

    /// The entry with `field` set back to a prior value recorded by [`field_changes`], or `None`
    /// for a value this module did not record.
    pub fn with_prior(self, field: &str, value: &Value) -> Option<Self> {
        Some(match field {
            "saved" => Self {
                saved: value.as_bool()?,
                ..self
            },
            "status" => Self {
                status: Status::parse(value.as_str()?)?,
                ..self
            },
            "saved_at" => Self {
                saved_at: match value {
                    Value::Null => None,
                    value => Some(DateTime::parse_from_rfc3339(value.as_str()?).ok()?.to_utc()),
                },
                ..self
            },
            _ => return None,
        })
    }

    /// The entry after an episode is marked watched (C05): the show is saved and Plan to watch
    /// becomes Watching; On hold and Dropped are kept.
    pub fn watched(self, now: DateTime<Utc>) -> Self {
        Self {
            status: match self.status {
                Status::PlanToWatch => Status::Watching,
                status => status,
            },
            ..self.saved(now)
        }
    }
}

/// A library entry locked `FOR UPDATE` for the rest of the transaction.
pub struct LockedEntry {
    user_id: Uuid,
    show_id: Uuid,
    exists: bool,
    pub before: Entry,
    /// 0 when the entry has no row.
    pub revision: i64,
}

impl LockedEntry {
    pub async fn lock(
        conn: &mut PgConnection,
        user_id: Uuid,
        show_id: Uuid,
    ) -> Result<Self, ApiError> {
        let row: Option<(bool, String, Option<DateTime<Utc>>, i64)> = sqlx::query_as(
            "SELECT saved, status, saved_at, revision FROM library_entries
             WHERE user_id = $1 AND show_id = $2 FOR UPDATE",
        )
        .bind(user_id)
        .bind(show_id)
        .fetch_optional(&mut *conn)
        .await?;
        let (exists, before, revision) = match row {
            Some((saved, status, saved_at, revision)) => (
                true,
                Entry {
                    saved,
                    status: Status::parse(&status)
                        .ok_or_else(|| ApiError::unavailable("status"))?,
                    saved_at,
                },
                revision,
            ),
            None => (false, ABSENT, 0),
        };
        Ok(Self {
            user_id,
            show_id,
            exists,
            before,
            revision,
        })
    }

    /// Writes `after` at the next revision and returns the changed fields for the action
    /// recorder. Writes nothing and returns no changes when `after` equals the current state.
    pub async fn store(
        &self,
        conn: &mut PgConnection,
        after: Entry,
    ) -> Result<Vec<Change>, ApiError> {
        if after == self.before {
            return Ok(Vec::new());
        }
        let new_revision: Option<i64> = if self.exists {
            sqlx::query_scalar(
                "UPDATE library_entries SET saved = $3, status = $4, saved_at = $5,
                     revision = revision + 1, updated_at = now()
                 WHERE user_id = $1 AND show_id = $2 RETURNING revision",
            )
        } else {
            // A concurrent first save makes this a conflict rather than a second insert.
            sqlx::query_scalar(
                "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
                 VALUES ($1, $2, $3, $4, $5, 1) ON CONFLICT DO NOTHING RETURNING revision",
            )
        }
        .bind(self.user_id)
        .bind(self.show_id)
        .bind(after.saved)
        .bind(after.status.as_str())
        .bind(after.saved_at)
        .fetch_optional(&mut *conn)
        .await?;
        let new_revision =
            new_revision.ok_or_else(|| ApiError::new(ErrorCode::RevisionConflict))?;
        Ok(field_changes(
            self.show_id,
            &self.before,
            &after,
            new_revision,
        ))
    }
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

    #[test]
    fn prior_values_restore_the_recorded_entry() {
        let at = Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap()
            + chrono::Duration::microseconds(123_456);
        let before = Entry {
            saved: true,
            status: Status::OnHold,
            saved_at: Some(at),
        };
        let after = Entry {
            saved: false,
            status: Status::Dropped,
            saved_at: None,
        };
        let restored = field_changes(Uuid::nil(), &before, &after, 2)
            .iter()
            .try_fold(after, |entry, change| {
                entry.with_prior(change.field, &change.before)
            });
        assert_eq!(restored, Some(before));
        assert_eq!(ABSENT.with_prior("saved", &json!("yes")), None);
        assert_eq!(ABSENT.with_prior("revision", &json!(1)), None);
    }

    #[test]
    fn watching_saves_and_upgrades_only_plan_to_watch() {
        let now = Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap();
        let earlier = now - chrono::Duration::days(1);
        assert_eq!(
            ABSENT.watched(now),
            Entry {
                saved: true,
                status: Status::Watching,
                saved_at: Some(now),
            }
        );
        for status in [Status::OnHold, Status::Dropped, Status::Watching] {
            let saved = Entry {
                saved: true,
                status,
                saved_at: Some(earlier),
            };
            assert_eq!(saved.watched(now), saved);
            let removed = Entry {
                saved: false,
                ..saved
            };
            assert_eq!(
                removed.watched(now),
                Entry {
                    saved_at: Some(now),
                    ..saved
                }
            );
        }
    }
}
