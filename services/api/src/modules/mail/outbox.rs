//! `outbox` persistence (0006). A message is due while `delivered_at IS NULL`, `available_at <=
//! now()` and attempts remain. Claiming increments `attempts` and pushes `available_at` past the
//! lease, so a worker that crashes mid-send releases the message when the lease ends and every
//! claim, including a crashed one, counts toward the bounded retries.

use std::time::Duration;

use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

/// Claims per message, including the first. An exhausted message stays undelivered for review.
pub const MAX_ATTEMPTS: i32 = 8;
/// How long a claim is held; longer than the SMTP timeout so a live send is not reclaimed.
pub const LEASE: Duration = Duration::from_secs(60);

const FIRST_RETRY: Duration = Duration::from_secs(30);
const MAX_RETRY: Duration = Duration::from_secs(60 * 60);

/// A claimed message. `payload` holds identifiers only, never secrets or message bodies.
#[derive(Clone, Debug)]
pub struct Claimed {
    pub id: Uuid,
    pub kind: String,
    pub payload: Value,
    /// Claims so far, including this one.
    pub attempts: i32,
}

/// Queues a message in the caller's transaction. A `dedupe_key` that is already queued or
/// delivered is not queued again, so a repeated request does not send twice; one whose message
/// exhausted its retries is revived with a fresh retry budget. Returns whether anything changed.
pub async fn enqueue(
    conn: &mut PgConnection,
    kind: &str,
    dedupe_key: &str,
    payload: &Value,
) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "INSERT INTO outbox (kind, dedupe_key, payload) VALUES ($1, $2, $3)
         ON CONFLICT (dedupe_key) DO UPDATE SET attempts = 0, available_at = now()
         WHERE outbox.delivered_at IS NULL AND outbox.attempts >= $4
           AND outbox.available_at <= now()",
    )
    .bind(kind)
    .bind(dedupe_key)
    .bind(payload)
    .bind(MAX_ATTEMPTS)
    .execute(conn)
    .await
    .map(|result| result.rows_affected() == 1)
}

/// Leases up to `limit` due messages for `lease`. Concurrent workers skip each other's rows.
pub async fn claim(
    pool: &PgPool,
    limit: i64,
    lease: Duration,
) -> Result<Vec<Claimed>, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE outbox o
         SET attempts = o.attempts + 1, available_at = now() + make_interval(secs => $2)
         FROM (
             SELECT id FROM outbox
             WHERE delivered_at IS NULL AND available_at <= now() AND attempts < $3
             ORDER BY available_at, id
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         ) due
         WHERE o.id = due.id
         RETURNING o.id, o.kind, o.payload, o.attempts",
    )
    .bind(limit)
    .bind(lease.as_secs_f64())
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(Claimed {
                id: row.try_get("id")?,
                kind: row.try_get("kind")?,
                payload: row.try_get("payload")?,
                attempts: row.try_get("attempts")?,
            })
        })
        .collect()
}

/// Settles a message: sent, or nothing left to send.
pub async fn settle(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outbox SET delivered_at = now() WHERE id = $1 AND delivered_at IS NULL")
        .bind(id)
        .execute(pool)
        .await
        .map(drop)
}

/// Releases a failed claim to retry after `delay`. A claim whose lease was already taken over
/// (attempts moved on) is left to its new holder.
pub async fn retry_later(
    pool: &PgPool,
    claimed: &Claimed,
    delay: Duration,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE outbox SET available_at = now() + make_interval(secs => $3)
         WHERE id = $1 AND attempts = $2 AND delivered_at IS NULL",
    )
    .bind(claimed.id)
    .bind(claimed.attempts)
    .bind(delay.as_secs_f64())
    .execute(pool)
    .await
    .map(drop)
}

/// Exponential backoff after the `attempts`-th failed claim: 30s, 1m, 2m, … capped at 1h.
pub fn backoff(attempts: i32) -> Duration {
    let doublings = attempts.saturating_sub(1).clamp(0, 16) as u32;
    FIRST_RETRY.saturating_mul(1 << doublings).min(MAX_RETRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(2), Duration::from_secs(60));
        assert_eq!(backoff(3), Duration::from_secs(120));
        assert_eq!(backoff(8), Duration::from_secs(3600));
        assert_eq!(backoff(i32::MAX), Duration::from_secs(3600));
        assert_eq!(backoff(0), Duration::from_secs(30));
    }
}
