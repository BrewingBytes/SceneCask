//! Hosted episode discussions (C07), relative to `/api/v1`. [`threads`] lists and creates
//! host-owned threads on an episode and [`reads`] pages a thread's comments. Integration (R38)
//! mounts [`routes`] only while the beta flag is on. Every request checks the privacy gate in
//! [`crate::domain::access`] against current edges first, then the spoiler gate in
//! [`crate::domain::spoilers`] with the viewer's own marks and this session's discussion grants.

pub mod reads;
pub mod threads;

use axum::{
    Router,
    extract::{Query, rejection::QueryRejection},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgArguments, query::QueryAs};
use uuid::Uuid;

use crate::{
    error::ApiError,
    middleware::Security,
    modules::library::{decode_opaque, encode_opaque, page_limit},
};

/// Every R30 route, for the beta router.
pub fn routes(security: Security) -> Router {
    Router::new()
        .merge(threads::routes(security.clone()))
        .merge(reads::routes(security))
}

const DEFAULT_LIMIT: i64 = 20;

#[derive(Debug, Deserialize)]
struct PageParams {
    cursor: Option<String>,
    limit: Option<String>,
}

/// A keyset position: a thread's or comment's createdAt and ID.
type Position = (DateTime<Utc>, Uuid);

type PgQueryAs<'q, O> = QueryAs<'q, Postgres, O, PgArguments>;

/// A validated page request: the row limit and the position to continue after.
struct Page {
    limit: i64,
    after: Option<Position>,
}

impl Page {
    fn parse(params: Result<Query<PageParams>, QueryRejection>) -> Result<Self, ApiError> {
        let Query(params) = params.map_err(|_| ApiError::malformed())?;
        let parse = |text: &str| {
            let (micros, id) = text.split_once('.')?;
            let at = DateTime::<Utc>::from_timestamp_micros(micros.parse().ok()?)?;
            Some((at, Uuid::try_parse(id).ok()?))
        };
        Ok(Self {
            limit: page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?,
            after: params
                .cursor
                .map(|cursor| decode_opaque(&cursor, parse))
                .transpose()?,
        })
    }

    /// Binds the position to `$3`/`$4` and the limit plus one lookahead row to `$5`.
    fn bind<'q, O>(&self, query: PgQueryAs<'q, O>) -> PgQueryAs<'q, O> {
        query
            .bind(self.after.map(|(created_at, _)| created_at))
            .bind(self.after.map(|(_, id)| id))
            .bind(self.limit + 1)
    }

    /// Drops the lookahead row and returns the cursor after the last kept row, if more exist.
    fn split<R>(&self, rows: &mut Vec<R>, position: impl Fn(&R) -> Position) -> Option<String> {
        let limit = self.limit as usize;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let (created_at, id) = rows.last().filter(|_| more).map(position)?;
        Some(encode_opaque(&format!(
            "{}.{}",
            created_at.timestamp_micros(),
            id.simple()
        )))
    }
}

/// A read-only snapshot, so membership, the spoiler gate and the rows they guard agree even when
/// a block, unfollow or mark commits concurrently.
async fn snapshot(pool: &PgPool) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// A path ID that is not a UUID cannot name an episode or thread: 404, like an unknown one.
fn path_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(value).map_err(|_| ApiError::not_found())
}
