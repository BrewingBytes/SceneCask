//! Session-scoped reveal grants (0007 `reveal_grants`). A grant belongs to one session, user,
//! scope and resource; deleting the session (logout, reset, expiry, disabling) cascades to it.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    domain::spoilers::{Grant, RevealScope, Session},
    modules::auth::session::CurrentSession,
};

/// The request's session as the spoiler policy sees it. Owns the session hash so projections can
/// borrow a [`Session`] from it.
pub struct SessionKey {
    id_hash: Vec<u8>,
    user_id: Uuid,
}

impl SessionKey {
    pub fn of(current: &CurrentSession) -> Self {
        Self {
            id_hash: current.id_hash(),
            user_id: current.user.id,
        }
    }

    pub fn session(&self) -> Session<'_> {
        Session {
            id_hash: &self.id_hash,
            user_id: self.user_id,
        }
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
}

/// A stored grant, convertible into the policy's [`Grant`].
pub struct StoredGrant {
    session_id: Vec<u8>,
    user_id: Uuid,
    scope: RevealScope,
    resource_id: Uuid,
    expires_at: DateTime<Utc>,
}

impl StoredGrant {
    pub fn grant(&self) -> Grant<'_> {
        Grant {
            session_id: &self.session_id,
            user_id: self.user_id,
            scope: self.scope,
            resource_id: self.resource_id,
            expires_at: self.expires_at,
        }
    }
}

/// This session's unexpired `scope` grants for any of `resource_ids`.
pub async fn live(
    conn: &mut PgConnection,
    key: &SessionKey,
    scope: RevealScope,
    resource_ids: &[Uuid],
) -> Result<Vec<StoredGrant>, sqlx::Error> {
    if resource_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT resource_id, expires_at FROM reveal_grants
         WHERE session_id = $1 AND user_id = $2 AND scope = $3 AND resource_id = ANY($4)
           AND expires_at > now()",
    )
    .bind(&key.id_hash)
    .bind(key.user_id)
    .bind(scope.as_str())
    .bind(resource_ids)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(resource_id, expires_at)| StoredGrant {
            session_id: key.id_hash.clone(),
            user_id: key.user_id,
            scope,
            resource_id,
            expires_at,
        })
        .collect())
}

/// Creates or renews this session's grant; it expires after 12 hours or with the session's
/// absolute expiry, whichever is first. `None` when the session no longer exists.
pub async fn create(
    conn: &mut PgConnection,
    key: &SessionKey,
    scope: RevealScope,
    resource_id: Uuid,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    // The 0007 trigger stamps created_at with clock_timestamp() after this expression runs, so the
    // table's 12-hour bound holds.
    sqlx::query_scalar(
        "INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
         SELECT id_hash, user_id, $3, $4, LEAST(clock_timestamp() + interval '12 hours', expires_at)
         FROM sessions
         WHERE id_hash = $1 AND user_id = $2 AND expires_at > clock_timestamp()
         ON CONFLICT (session_id, scope, resource_id)
         DO UPDATE SET expires_at = EXCLUDED.expires_at, created_at = clock_timestamp()
         RETURNING expires_at",
    )
    .bind(&key.id_hash)
    .bind(key.user_id)
    .bind(scope.as_str())
    .bind(resource_id)
    .fetch_optional(conn)
    .await
}

/// Hide again: removes this session's grant, if any.
pub async fn delete(
    conn: &mut PgConnection,
    key: &SessionKey,
    scope: RevealScope,
    resource_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM reveal_grants
         WHERE session_id = $1 AND user_id = $2 AND scope = $3 AND resource_id = $4",
    )
    .bind(&key.id_hash)
    .bind(key.user_id)
    .bind(scope.as_str())
    .bind(resource_id)
    .execute(conn)
    .await?;
    Ok(())
}
