//! Password credential and reset persistence. Emails are matched with the same full casefold as
//! `users.normalized_email` (0001); the caller has already trimmed them.

use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::modules::auth::{
    credentials::NORMALIZED,
    email_token::{self, RESET},
    session::store::revoke_user,
};

/// An enabled account with a password. The hash is a credential: never log it.
pub(super) struct LoginAccount {
    pub id: Uuid,
    pub verified: bool,
    pub argon2_hash: String,
}

/// The enabled account for `email` and its password hash, or `None` when there is no such
/// account or it has no password (Google only).
pub(super) async fn login_account(
    pool: &PgPool,
    email: &str,
) -> Result<Option<LoginAccount>, sqlx::Error> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT u.id, u.verified_at IS NOT NULL AS verified, p.argon2_hash
         FROM users u JOIN password_credentials p ON p.user_id = u.id
         WHERE u.normalized_email = {NORMALIZED} AND u.disabled_at IS NULL"
    )))
    .bind(email)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(LoginAccount {
            id: row.try_get("id")?,
            verified: row.try_get("verified")?,
            argon2_hash: row.try_get("argon2_hash")?,
        })
    })
    .transpose()
}

/// The password hash of an enabled account, or `None` (Google only, or disabled meanwhile).
pub(super) async fn password_hash(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT p.argon2_hash FROM password_credentials p JOIN users u ON u.id = p.user_id
         WHERE p.user_id = $1 AND u.disabled_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Queues a reset email for the enabled account of `email` outside its cooldown, invalidating
/// its earlier reset links; otherwise does nothing. Concurrent requests serialize on the row lock
/// and queue one email.
pub(super) async fn request_reset(pool: &PgPool, email: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let user_id: Option<Uuid> = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT id FROM users WHERE normalized_email = {NORMALIZED} AND disabled_at IS NULL
         FOR UPDATE"
    )))
    .bind(email)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(user_id) = user_id
        && !RESET.cooling_down(&mut tx, user_id).await?
    {
        RESET.queue(&mut tx, user_id).await?;
    }
    tx.commit().await
}

/// Consumes a live reset link and, in the caller's transaction: sets the password (creating one
/// for a Google-only account), marks the email verified, invalidates every other emailed link and
/// deletes every session, which revokes their reveal grants and OAuth flows. Returns the account,
/// or `None` for an unknown, used, superseded or expired link, leaving everything unchanged.
pub(super) async fn reset(
    conn: &mut PgConnection,
    token_hash: &[u8],
    argon2_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let Some(user_id) = RESET.consume(&mut *conn, token_hash).await? else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET argon2_hash = excluded.argon2_hash,
                                             updated_at = now()",
    )
    .bind(user_id)
    .bind(argon2_hash)
    .execute(&mut *conn)
    .await?;
    email_token::mark_verified(&mut *conn, user_id).await?;
    RESET.invalidate(&mut *conn, user_id).await?;
    revoke_user(&mut *conn, user_id).await?;
    Ok(Some(user_id))
}
