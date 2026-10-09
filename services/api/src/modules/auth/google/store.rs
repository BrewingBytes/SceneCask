//! Google identity persistence. An identity is `(issuer, subject)` (0001 unique); the email claim
//! only seeds a new account and never selects or merges one.

use sqlx::{AssertSqlSafe, PgConnection};
use uuid::Uuid;

use crate::modules::auth::credentials::NORMALIZED;

/// Serializes callbacks for one identity until the transaction ends, so concurrent sign-ins and
/// links of a new subject create or link it once. Taken before any account lock.
pub(super) async fn lock_identity(
    conn: &mut PgConnection,
    issuer: &str,
    subject: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1 || chr(10) || $2, 0))")
        .bind(issuer)
        .bind(subject)
        .execute(conn)
        .await
        .map(drop)
}

/// The account that owns the identity, if any.
pub(super) async fn owner(
    conn: &mut PgConnection,
    issuer: &str,
    subject: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT user_id FROM external_identities WHERE issuer = $1 AND subject = $2")
        .bind(issuer)
        .bind(subject)
        .fetch_optional(conn)
        .await
}

pub(super) enum Created {
    Account(Uuid),
    /// Any account already uses the email: that requires explicit linking, never a merge.
    EmailTaken,
    /// Another account owns the identity by now. The caller must roll back, or the new account
    /// would remain without any sign-in method.
    IdentityTaken,
}

/// Creates a verified, private account (0001 defaults) owning the identity. `email` is trimmed;
/// PostgreSQL casefolds it like registration does.
pub(super) async fn create_account(
    conn: &mut PgConnection,
    email: &str,
    display_name: Option<&str>,
    issuer: &str,
    subject: &str,
) -> Result<Created, sqlx::Error> {
    let created: Option<Uuid> = sqlx::query_scalar(AssertSqlSafe(format!(
        "INSERT INTO users (normalized_email, display_name, verified_at)
         VALUES ({NORMALIZED}, $2, now())
         ON CONFLICT (normalized_email) DO NOTHING RETURNING id"
    )))
    .bind(email)
    .bind(display_name)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(user_id) = created else {
        return Ok(Created::EmailTaken);
    };
    Ok(if insert(conn, user_id, issuer, subject).await? {
        Created::Account(user_id)
    } else {
        Created::IdentityTaken
    })
}

/// Locks the account row, which linking, unlinking and Google reauth all take (linking right
/// after [`lock_identity`]).
pub(crate) async fn lock_account(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .execute(conn)
        .await
        .map(drop)
}

pub(super) enum Link {
    Linked,
    /// This account already owns this identity; linking again changes nothing.
    AlreadyLinked,
    /// Another account owns the identity, or this account has a different one from this issuer.
    InUse,
}

/// Links the identity to `user_id` under [`lock_identity`] and [`lock_account`], so it never
/// interleaves with a sign-in creating an account for the same identity, or with an unlink of
/// the same account.
pub(super) async fn link(
    conn: &mut PgConnection,
    user_id: Uuid,
    issuer: &str,
    subject: &str,
) -> Result<Link, sqlx::Error> {
    lock_identity(conn, issuer, subject).await?;
    lock_account(conn, user_id).await?;
    if insert(conn, user_id, issuer, subject).await? {
        return Ok(Link::Linked);
    }
    Ok(match owner(conn, issuer, subject).await? {
        Some(owner) if owner == user_id => Link::AlreadyLinked,
        _ => Link::InUse,
    })
}

/// Inserts the identity unless either uniqueness (issuer+subject, or one per account and issuer)
/// is taken; a concurrent insert of the same key waits for the other transaction.
async fn insert(
    conn: &mut PgConnection,
    user_id: Uuid,
    issuer: &str,
    subject: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "INSERT INTO external_identities (user_id, issuer, subject) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(issuer)
    .bind(subject)
    .execute(conn)
    .await
    .map(|result| result.rows_affected() == 1)
}
