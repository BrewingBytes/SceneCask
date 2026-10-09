//! C03 idempotency for progress, library, catch-up and history writes, shared with the tracking
//! mutation service (R16). Each write runs in one transaction that first calls [`claim`], then
//! performs its domain write and records the action, then calls [`Claim::store`], so the stored
//! response commits atomically with the write it describes. [`transact`] runs that sequence.

use axum::{
    extract::FromRequestParts,
    http::{HeaderName, request::Parts},
};
use chrono::Duration;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::{ApiError, ErrorCode};

pub const IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");

/// C03: records are retained for 24 hours.
pub const RETENTION: Duration = Duration::hours(24);

/// The required `Idempotency-Key` UUID header; absent, repeated or malformed is 400.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdempotencyKey(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let mut values = parts.headers.get_all(IDEMPOTENCY_HEADER).iter();
        match (values.next(), values.next()) {
            (Some(value), None) => value
                .to_str()
                .ok()
                .and_then(|text| Uuid::try_parse(text.trim()).ok())
                .map(Self)
                .ok_or_else(ApiError::malformed),
            _ => Err(ApiError::malformed()),
        }
    }
}

/// What a request means: the operation, its target and its parsed body. Equal fingerprints are
/// the same request, regardless of JSON whitespace or key order.
pub fn fingerprint(operation: &str, target: Uuid, body: &impl Serialize) -> Vec<u8> {
    let body = serde_json::to_value(body).unwrap_or(Value::Null);
    let mut hash = Sha256::new();
    hash.update(operation.as_bytes());
    hash.update([0]);
    hash.update(target.as_bytes());
    hash.update(body.to_string().as_bytes());
    hash.finalize().to_vec()
}

/// An unused key, held until the transaction ends.
#[must_use = "store the response before committing"]
pub struct Claim {
    user_id: Uuid,
    key: Uuid,
    request_hash: Vec<u8>,
}

/// The outcome of claiming a key.
pub enum Claimed {
    /// First use (or the earlier record expired): perform the write, then store its response.
    New(Claim),
    /// The same request already succeeded: return this stored response unchanged.
    Replay(Value),
}

/// Serializes requests using `key` for this user until the transaction ends, then looks the key
/// up. A live record for a different request is 409 REVISION_CONFLICT. Call before taking any
/// row lock, so every writer acquires the key first.
pub async fn claim(
    conn: &mut PgConnection,
    user_id: Uuid,
    key: IdempotencyKey,
    request_hash: Vec<u8>,
) -> Result<Claimed, ApiError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('idempotency.' || $1::text || '.' || $2::text, 0))")
        .bind(user_id)
        .bind(key.0)
        .execute(&mut *conn)
        .await?;
    let stored: Option<(Vec<u8>, sqlx::types::Json<Value>)> = sqlx::query_as(
        "SELECT request_hash, response FROM idempotency_records
         WHERE user_id = $1 AND key = $2 AND expires_at > now()",
    )
    .bind(user_id)
    .bind(key.0)
    .fetch_optional(&mut *conn)
    .await?;
    match stored {
        Some((hash, response)) if hash == request_hash => Ok(Claimed::Replay(response.0)),
        Some(_) => Err(ApiError::new(ErrorCode::RevisionConflict)),
        None => Ok(Claimed::New(Claim {
            user_id,
            key: key.0,
            request_hash,
        })),
    }
}

impl Claim {
    /// Stores the successful response, replacing an expired record for the same key.
    pub async fn store(
        self,
        conn: &mut PgConnection,
        response: &impl Serialize,
    ) -> Result<Value, ApiError> {
        let response =
            serde_json::to_value(response).map_err(|_| ApiError::unavailable("idempotency"))?;
        sqlx::query(
            "INSERT INTO idempotency_records (user_id, key, request_hash, response, expires_at)
             VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))
             ON CONFLICT (user_id, key) DO UPDATE SET request_hash = EXCLUDED.request_hash,
                 response = EXCLUDED.response, created_at = now(), expires_at = EXCLUDED.expires_at",
        )
        .bind(self.user_id)
        .bind(self.key)
        .bind(&self.request_hash)
        .bind(sqlx::types::Json(&response))
        .bind(RETENTION.num_seconds() as f64)
        .execute(conn)
        .await?;
        Ok(response)
    }
}

/// Runs `write` in one transaction keyed by `key`: claims the key, writes, stores the response
/// and commits. A replayed key returns the stored response without calling `write`; any error
/// rolls back every write, including the idempotency record.
pub async fn transact<T: Serialize>(
    pool: &PgPool,
    user_id: Uuid,
    key: IdempotencyKey,
    request_hash: Vec<u8>,
    write: impl AsyncFnOnce(&mut PgConnection) -> Result<T, ApiError>,
) -> Result<Value, ApiError> {
    let mut tx = pool.begin().await?;
    let claim = match claim(&mut tx, user_id, key, request_hash).await? {
        Claimed::Replay(response) => return Ok(response),
        Claimed::New(claim) => claim,
    };
    let result = write(&mut tx).await?;
    let response = claim.store(&mut tx, &result).await?;
    tx.commit().await?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn fingerprint_ignores_key_order_but_not_target_or_values() {
        let show = Uuid::from_u128(1);
        let a = fingerprint(
            "library.save",
            show,
            &json!({"saved": true, "expectedRevision": 0}),
        );
        let b = fingerprint(
            "library.save",
            show,
            &json!({"expectedRevision": 0, "saved": true}),
        );
        assert_eq!(a, b);
        for other in [
            fingerprint(
                "library.save",
                Uuid::from_u128(2),
                &json!({"saved": true, "expectedRevision": 0}),
            ),
            fingerprint(
                "library.status",
                show,
                &json!({"saved": true, "expectedRevision": 0}),
            ),
            fingerprint(
                "library.save",
                show,
                &json!({"saved": false, "expectedRevision": 0}),
            ),
        ] {
            assert_ne!(a, other);
        }
    }
}
