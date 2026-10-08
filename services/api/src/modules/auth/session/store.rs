//! `sessions` persistence. Expiry is evaluated with the database clock: a session is valid while
//! `now() < expires_at` (absolute) and `now() < last_seen_at + idle`. Deleting a session cascades
//! to its reveal grants and OAuth flows (0001/0007 foreign keys).

use sqlx::{AssertSqlSafe, PgConnection, PgExecutor, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use super::{Lifetimes, Role, SessionUser, secret::Secret};
use crate::error::{ApiError, ErrorCode};

/// Columns read by [`user_from_row`]; `u` is the users table.
const USER_COLUMNS: &str = "u.id, u.normalized_email, u.display_name, u.handle, u.visibility,
    u.verified_at IS NOT NULL AS verified, u.role";

fn user_from_row(row: &PgRow) -> Result<SessionUser, sqlx::Error> {
    let role: String = row.try_get("role")?;
    Ok(SessionUser {
        id: row.try_get("id")?,
        email: row.try_get("normalized_email")?,
        display_name: row.try_get("display_name")?,
        handle: row.try_get("handle")?,
        visibility: row.try_get("visibility")?,
        verified: row.try_get("verified")?,
        role: if role == "operator" {
            Role::Operator
        } else {
            Role::Member
        },
    })
}

pub(super) struct Found {
    pub user: SessionUser,
    pub reauthenticated_secs_ago: Option<f64>,
}

/// Loads the live session for `hash` and its enabled user, refreshing `last_seen_at` at most
/// once per touch interval.
pub(super) async fn lookup(
    pool: &PgPool,
    hash: &[u8],
    lifetimes: &Lifetimes,
) -> Result<Option<Found>, sqlx::Error> {
    let sql = format!(
        "WITH live AS (
             SELECT id_hash, user_id, last_seen_at, reauthenticated_at FROM sessions
             WHERE id_hash = $1 AND expires_at > now()
               AND last_seen_at > now() - make_interval(secs => $2)
         ), touched AS (
             UPDATE sessions SET last_seen_at = now() FROM live
             WHERE sessions.id_hash = live.id_hash
               AND live.last_seen_at < now() - make_interval(secs => $3)
         )
         SELECT {USER_COLUMNS},
                extract(epoch FROM now() - live.reauthenticated_at)::float8
                    AS reauthenticated_secs_ago
         FROM live JOIN users u ON u.id = live.user_id
         WHERE u.disabled_at IS NULL"
    );
    let Some(row) = sqlx::query(AssertSqlSafe(sql))
        .bind(hash)
        .bind(lifetimes.idle.as_secs_f64())
        .bind(lifetimes.touch_interval.as_secs_f64())
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Found {
        user: user_from_row(&row)?,
        reauthenticated_secs_ago: row.try_get("reauthenticated_secs_ago")?,
    }))
}

/// Creates a session for an enabled, verified user and returns its secret and user. The user is
/// checked before `previous` (the session being rotated) is revoked, so a refused rotation keeps
/// the caller signed in. Pass a transaction to make the rotation atomic with the caller's writes.
pub(super) async fn create(
    conn: &mut PgConnection,
    user_id: Uuid,
    previous: Option<&Secret>,
    lifetimes: &Lifetimes,
) -> Result<(Secret, SessionUser), ApiError> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT {USER_COLUMNS} FROM users u WHERE u.id = $1 AND u.disabled_at IS NULL"
    )))
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let user = user_from_row(&row.ok_or_else(ApiError::auth_required)?)?;
    if !user.verified {
        return Err(ApiError::new(ErrorCode::EmailUnverified));
    }
    let secret = Secret::generate()?;
    if let Some(previous) = previous {
        revoke(&mut *conn, &previous.hash()).await?;
    }
    sqlx::query(
        "INSERT INTO sessions (id_hash, user_id, expires_at, reauthenticated_at)
         VALUES ($1, $2, now() + make_interval(secs => $3), now())",
    )
    .bind(secret.hash())
    .bind(user_id)
    .bind(lifetimes.absolute.as_secs_f64())
    .execute(&mut *conn)
    .await?;
    Ok((secret, user))
}

/// Deletes one session (and, by cascade, its reveal grants). Missing sessions are not an error.
pub async fn revoke<'e>(executor: impl PgExecutor<'e>, hash: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
        .bind(hash)
        .execute(executor)
        .await
        .map(drop)
}

/// Deletes every session and reveal grant of `user_id` (password reset, account deletion).
pub async fn revoke_user<'e>(
    executor: impl PgExecutor<'e>,
    user_id: Uuid,
) -> Result<u64, sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(executor)
        .await
        .map(|result| result.rows_affected())
}

/// Deletes idle- or absolute-expired sessions and their grants; for a periodic worker.
pub async fn purge_expired(pool: &PgPool, lifetimes: &Lifetimes) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "DELETE FROM sessions
         WHERE expires_at <= now() OR last_seen_at <= now() - make_interval(secs => $1)",
    )
    .bind(lifetimes.idle.as_secs_f64())
    .execute(pool)
    .await
    .map(|result| result.rows_affected())
}

/// Replaces the live session `hash` with a new secret after reauthentication (C04: rotate on
/// authentication), so a copy of the old cookie cannot inherit the fresh reauthentication. The
/// sign-in lifetime is unchanged: `created_at` and the absolute `expires_at` carry over, and the
/// session's reveal grants move to the new secret. Returns the new secret and the seconds until
/// absolute expiry, or `None` if the session is no longer live. Call within a transaction.
pub(super) async fn rotate_reauthenticated(
    conn: &mut PgConnection,
    hash: &[u8],
) -> Result<Option<(Secret, f64)>, ApiError> {
    let secret = Secret::generate()?;
    let new_hash = secret.hash();
    let remaining: Option<f64> = sqlx::query_scalar(
        "INSERT INTO sessions (id_hash, user_id, created_at, last_seen_at, expires_at,
                               reauthenticated_at)
         SELECT $2, user_id, created_at, now(), expires_at, now() FROM sessions
         WHERE id_hash = $1 AND expires_at > now()
         RETURNING extract(epoch FROM expires_at - now())::float8",
    )
    .bind(hash)
    .bind(&new_hash)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(remaining) = remaining else {
        return Ok(None);
    };
    sqlx::query("UPDATE reveal_grants SET session_id = $2 WHERE session_id = $1")
        .bind(hash)
        .bind(&new_hash)
        .execute(&mut *conn)
        .await?;
    revoke(&mut *conn, hash).await?;
    Ok(Some((secret, remaining)))
}
