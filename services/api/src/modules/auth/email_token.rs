//! Single-use emailed links in `auth_tokens` (0001): email verification and password reset.
//!
//! A request queues an outbox message naming only the account. The token is minted when that
//! email is composed ([`EmailToken::mint`]), so the outbox never holds one, and each mint
//! invalidates the account's earlier links of the same purpose: only the newest sent link works.
//! A cooldown keeps requests from flooding one mailbox.

use std::time::Duration;

use serde_json::json;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};
use uuid::Uuid;

use crate::modules::mail::{
    outbox,
    template::{PASSWORD_RESET_EMAIL, Template, VERIFY_EMAIL},
};

pub struct EmailToken {
    /// `auth_tokens.purpose`.
    purpose: &'static str,
    /// Outbox kind of the email.
    pub kind: &'static str,
    pub lifetime: Duration,
    /// Minimum gap between two emails of this kind to one account.
    pub cooldown: Duration,
    /// Accounts that may still receive this link: a condition on `users`.
    eligible: &'static str,
    pub(super) template: Template,
    /// Web route the link opens. The token travels in the fragment, which browsers never send to
    /// a server or in a Referer.
    pub(super) path: &'static str,
}

/// C04: verification links are valid for 24 hours, at most one email per minute per account.
pub const VERIFY: EmailToken = EmailToken {
    purpose: "verify",
    kind: "auth.verify_email",
    lifetime: Duration::from_secs(24 * 60 * 60),
    cooldown: Duration::from_secs(60),
    eligible: "verified_at IS NULL AND disabled_at IS NULL",
    template: VERIFY_EMAIL,
    path: "/auth/verify",
};

/// C04: reset links are valid for 30 minutes. Any enabled account may reset, including one with
/// only Google sign-in (establishing a password) or one still pending verification: using the
/// link proves control of the mailbox, as a verification link does.
pub const RESET: EmailToken = EmailToken {
    purpose: "reset",
    kind: "auth.password_reset",
    lifetime: Duration::from_secs(30 * 60),
    cooldown: Duration::from_secs(60),
    eligible: "disabled_at IS NULL",
    template: PASSWORD_RESET_EMAIL,
    path: "/auth/reset/confirm",
};

/// Every kind the auth composer sends.
pub(super) const ALL: [&EmailToken; 2] = [&VERIFY, &RESET];

impl EmailToken {
    /// Whether an email of this kind is still on its way (queued, leased or retrying) or one was
    /// sent within the cooldown. Call with the user row locked.
    pub(super) async fn cooling_down(
        &self,
        conn: &mut PgConnection,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS (
                        SELECT 1 FROM outbox
                        WHERE kind = $2 AND payload->>'userId' = $1::text AND delivered_at IS NULL
                          AND (attempts < $3 OR available_at > now()))
                 OR EXISTS (
                        SELECT 1 FROM auth_tokens
                        WHERE user_id = $1 AND purpose = $4
                          AND created_at > now() - make_interval(secs => $5))",
        )
        .bind(user_id)
        .bind(self.kind)
        .bind(outbox::MAX_ATTEMPTS)
        .bind(self.purpose)
        .bind(self.cooldown.as_secs_f64())
        .fetch_one(conn)
        .await
    }

    /// Invalidates outstanding links and queues one email. The dedupe key names the newest link
    /// this email supersedes, so a duplicate enqueue before the next link is minted collapses onto
    /// the queued one. Call with the user row locked.
    pub(super) async fn queue(
        &self,
        conn: &mut PgConnection,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        self.invalidate(&mut *conn, user_id).await?;
        let generation: i64 = sqlx::query_scalar(
            "SELECT coalesce((extract(epoch FROM max(created_at)) * 1000000)::bigint, 0)
             FROM auth_tokens WHERE user_id = $1 AND purpose = $2",
        )
        .bind(user_id)
        .bind(self.purpose)
        .fetch_one(&mut *conn)
        .await?;
        outbox::enqueue(
            conn,
            self.kind,
            &format!("{}:{user_id}:{generation}", self.kind),
            &json!({ "userId": user_id }),
        )
        .await
        .map(drop)
    }

    /// Marks every unused link of this purpose for `user_id` consumed.
    pub(super) async fn invalidate(
        &self,
        conn: &mut PgConnection,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE auth_tokens SET consumed_at = now()
             WHERE user_id = $1 AND purpose = $2 AND consumed_at IS NULL",
        )
        .bind(user_id)
        .bind(self.purpose)
        .execute(conn)
        .await
        .map(drop)
    }

    /// Stores `token_hash` as the only usable link of this purpose for an eligible account and
    /// returns the address to send it to, or `None` when the account is no longer eligible.
    pub(super) async fn mint(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        token_hash: &[u8],
    ) -> Result<Option<String>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let email: Option<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT normalized_email FROM users WHERE id = $1 AND {} FOR UPDATE",
            self.eligible
        )))
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        if email.is_some() {
            self.invalidate(&mut tx, user_id).await?;
            sqlx::query(
                "INSERT INTO auth_tokens (token_hash, user_id, purpose, expires_at)
                 VALUES ($1, $2, $3, now() + make_interval(secs => $4))",
            )
            .bind(token_hash)
            .bind(user_id)
            .bind(self.purpose)
            .bind(self.lifetime.as_secs_f64())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(email)
    }

    /// Consumes a live link of this purpose belonging to an enabled account and returns the
    /// account, or `None` for an unknown, used, invalidated or expired link. Concurrent attempts
    /// on one link serialize on its row, so exactly one succeeds. Call within the transaction
    /// that applies the link's effect.
    pub(super) async fn consume(
        &self,
        conn: &mut PgConnection,
        token_hash: &[u8],
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE auth_tokens t SET consumed_at = now()
             FROM users u
             WHERE t.token_hash = $1 AND t.purpose = $2 AND t.consumed_at IS NULL
               AND t.expires_at > now() AND u.id = t.user_id AND u.disabled_at IS NULL
             RETURNING t.user_id",
        )
        .bind(token_hash)
        .bind(self.purpose)
        .fetch_optional(conn)
        .await
    }
}

/// Marks the account's email verified: using any emailed link proves control of the mailbox.
/// Pending verification links become pointless and are invalidated.
pub(super) async fn mark_verified(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET verified_at = coalesce(verified_at, now()) WHERE id = $1")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    VERIFY.invalidate(conn, user_id).await
}
