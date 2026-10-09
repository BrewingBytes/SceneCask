//! Registration and verification persistence. PostgreSQL applies the same full casefold that
//! `users.normalized_email` is checked against (0001); the caller has already trimmed the email.
//! Verification links follow [`email_token`]: minted only when their email is sent.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::modules::auth::{
    credentials::NORMALIZED,
    email_token::{self, VERIFY},
};

/// Creates a pending account and queues its verification email, or queues a new email for a
/// pending one whose cooldown has passed. Verified, disabled and cooling-down accounts are left
/// unchanged. Concurrent registrations of one email serialize on the unique index and then on the
/// row lock, so they create one account and queue one email.
///
/// Once a link has been minted, a pending account's password is kept: the mailbox owner cannot
/// tell which registration a link confirms, so a later registrant must not be able to swap in
/// their own password before the owner clicks.
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
        None => match lock_pending(&mut tx, email).await? {
            Some(user_id) if !VERIFY.cooling_down(&mut tx, user_id).await? => Some(user_id),
            _ => None,
        },
    };
    if let Some(user_id) = user_id {
        // A pending password is replaced only while no link for it was ever minted.
        sqlx::query(
            "INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET argon2_hash = excluded.argon2_hash,
                                                 updated_at = now()
             WHERE NOT EXISTS (SELECT 1 FROM auth_tokens
                               WHERE user_id = $1 AND purpose = 'verify')",
        )
        .bind(user_id)
        .bind(argon2_hash)
        .execute(&mut *tx)
        .await?;
        VERIFY.queue(&mut tx, user_id).await?;
    }
    tx.commit().await
}

/// Queues a new verification email for a pending account outside its cooldown; otherwise does
/// nothing.
pub(super) async fn resend(pool: &PgPool, email: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if let Some(user_id) = lock_pending(&mut tx, email).await?
        && !VERIFY.cooling_down(&mut tx, user_id).await?
    {
        VERIFY.queue(&mut tx, user_id).await?;
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

/// Consumes a live verification token of an enabled account, marks the account verified and
/// invalidates its other links. Returns the account, or `None` for an unknown, used, invalidated
/// or expired token. Call within the transaction that starts the session.
pub(super) async fn consume_token(
    conn: &mut PgConnection,
    token_hash: &[u8],
) -> Result<Option<Uuid>, sqlx::Error> {
    let user_id = VERIFY.consume(&mut *conn, token_hash).await?;
    if let Some(user_id) = user_id {
        email_token::mark_verified(conn, user_id).await?;
    }
    Ok(user_id)
}
