//! Registration and verification persistence. PostgreSQL applies the same full casefold that
//! `users.normalized_email` is checked against (0001); the caller has already trimmed the email.
//! Verification tokens are minted only when their email is sent ([`mint_token`]), so the outbox
//! never holds one and only the newest sent link works.

use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{RESEND_COOLDOWN, TOKEN_LIFETIME, VERIFY_KIND};
use crate::modules::mail::outbox;

/// `$1` normalized as `users.normalized_email`. The result keeps the column's collation so the
/// unique index serves the lookup.
const NORMALIZED: &str = r#"casefold($1 COLLATE pg_unicode_fast) COLLATE "default""#;

/// Creates a pending account, or refreshes the password of a pending one whose cooldown has
/// passed, and queues its verification email. Verified, disabled and cooling-down accounts are
/// left unchanged. Concurrent registrations of one email serialize on the unique index and then
/// on the row lock, so they create one account and queue one email.
pub(super) async fn register(
    pool: &PgPool,
    email: &str,
    argon2_hash: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let created: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "INSERT INTO users (normalized_email) VALUES ({NORMALIZED})
         ON CONFLICT (normalized_email) DO NOTHING RETURNING id"
    )))
    .bind(email)
    .fetch_optional(&mut *tx)
    .await?;
    let user_id = match created {
        Some(user_id) => Some(user_id),
        // The latest registrant's password replaces the pending one only when a new link is
        // sent, and that link invalidates the earlier ones.
        None => match lock_pending(&mut tx, email).await? {
            Some(user_id) if !cooling_down(&mut tx, user_id).await? => Some(user_id),
            _ => None,
        },
    };
    if let Some(user_id) = user_id {
        sqlx::query(
            "INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET argon2_hash = excluded.argon2_hash,
                                                 updated_at = now()",
        )
        .bind(user_id)
        .bind(argon2_hash)
        .execute(&mut *tx)
        .await?;
        queue_verification(&mut tx, user_id).await?;
    }
    tx.commit().await
}

/// Queues a new verification email for a pending account outside its cooldown; otherwise does
/// nothing.
pub(super) async fn resend(pool: &PgPool, email: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if let Some(user_id) = lock_pending(&mut tx, email).await?
        && !cooling_down(&mut tx, user_id).await?
    {
        queue_verification(&mut tx, user_id).await?;
    }
    tx.commit().await
}

/// The unverified, enabled account for `email`, locked for this transaction.
async fn lock_pending(conn: &mut PgConnection, email: &str) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM users
         WHERE normalized_email = {NORMALIZED} AND verified_at IS NULL AND disabled_at IS NULL
         FOR UPDATE"
    )))
    .bind(email)
    .fetch_optional(conn)
    .await
}

/// Whether a verification email is still on its way (queued, leased or retrying) or one was sent
/// within the cooldown. Call with the user row locked.
async fn cooling_down(conn: &mut PgConnection, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
                    SELECT 1 FROM outbox
                    WHERE kind = $2 AND payload->>'userId' = $1::text AND delivered_at IS NULL
                      AND (attempts < $3 OR available_at > now()))
             OR EXISTS (
                    SELECT 1 FROM auth_tokens
                    WHERE user_id = $1 AND purpose = 'verify'
                      AND created_at > now() - make_interval(secs => $4))",
    )
    .bind(user_id)
    .bind(VERIFY_KIND)
    .bind(outbox::MAX_ATTEMPTS)
    .bind(RESEND_COOLDOWN.as_secs_f64())
    .fetch_one(conn)
    .await
}

/// Invalidates outstanding links and queues one verification email. The dedupe key names the
/// newest link this email supersedes, so a duplicate enqueue before the next link is minted
/// collapses onto the queued one.
async fn queue_verification(conn: &mut PgConnection, user_id: Uuid) -> Result<(), sqlx::Error> {
    invalidate_tokens(&mut *conn, user_id).await?;
    let generation: i64 = sqlx::query_scalar(
        "SELECT coalesce((extract(epoch FROM max(created_at)) * 1000000)::bigint, 0)
         FROM auth_tokens WHERE user_id = $1 AND purpose = 'verify'",
    )
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;
    outbox::enqueue(
        conn,
        VERIFY_KIND,
        &format!("{VERIFY_KIND}:{user_id}:{generation}"),
        &json!({ "userId": user_id }),
    )
    .await
    .map(drop)
}

async fn invalidate_tokens(conn: &mut PgConnection, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE auth_tokens SET consumed_at = now()
         WHERE user_id = $1 AND purpose = 'verify' AND consumed_at IS NULL",
    )
    .bind(user_id)
    .execute(conn)
    .await
    .map(drop)
}

/// Stores `token_hash` as the only usable verification link of a pending account and returns the
/// address to send it to, or `None` when the account no longer needs verification.
pub(super) async fn mint_token(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
) -> Result<Option<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let email: Option<String> = sqlx::query_scalar(
        "SELECT normalized_email FROM users
         WHERE id = $1 AND verified_at IS NULL AND disabled_at IS NULL FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    if email.is_some() {
        invalidate_tokens(&mut tx, user_id).await?;
        sqlx::query(
            "INSERT INTO auth_tokens (token_hash, user_id, purpose, expires_at)
             VALUES ($1, $2, 'verify', now() + make_interval(secs => $3))",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(TOKEN_LIFETIME.as_secs_f64())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(email)
}

/// Consumes a live verification token of an enabled account, marks the account verified and
/// invalidates its other links. Returns the account, or `None` for an unknown, used, invalidated
/// or expired token. Call within the transaction that starts the session.
pub(super) async fn consume_token(
    conn: &mut PgConnection,
    token_hash: &[u8],
) -> Result<Option<Uuid>, sqlx::Error> {
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE auth_tokens t SET consumed_at = now()
         FROM users u
         WHERE t.token_hash = $1 AND t.purpose = 'verify' AND t.consumed_at IS NULL
           AND t.expires_at > now() AND u.id = t.user_id AND u.disabled_at IS NULL
         RETURNING t.user_id",
    )
    .bind(token_hash)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(user_id) = user_id {
        sqlx::query("UPDATE users SET verified_at = coalesce(verified_at, now()) WHERE id = $1")
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
        invalidate_tokens(conn, user_id).await?;
    }
    Ok(user_id)
}
