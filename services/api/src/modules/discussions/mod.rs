//! Hosted episode discussions (C07), relative to `/api/v1`. [`threads`] lists and creates
//! host-owned threads on an episode, [`reads`] pages a thread's comments and [`comments`] posts
//! and removes them. Integration (R38)
//! mounts [`routes`] only while the beta flag is on. Every request checks the privacy gate in
//! [`crate::domain::access`] against current edges first, then the spoiler gate in
//! [`crate::domain::spoilers`] with the viewer's own marks and this session's discussion grants.

pub mod comments;
pub mod reads;
pub mod threads;

use axum::{
    Router,
    extract::{Query, rejection::QueryRejection},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{PgConnection, PgPool, Postgres, Transaction, postgres::PgArguments, query::QueryAs};
use uuid::Uuid;

use self::threads::EpisodeCatalog;
use crate::{
    domain::{access::DiscussionMember, spoilers::RevealScope},
    error::{ApiError, ErrorCode},
    middleware::Security,
    modules::{
        library::{decode_opaque, encode_opaque, page_limit},
        policy::{
            grants::{self, SessionKey},
            relationships,
        },
    },
};

/// Every R30 and R31 discussion route, for the beta router.
pub fn routes(security: Security) -> Router {
    Router::new()
        .merge(threads::routes(security.clone()))
        .merge(reads::routes(security.clone()))
        .merge(comments::routes(security))
}

/// Whether comment `c` may appear for the viewer bound to `$1`: not hidden or reported by them,
/// and not written by someone blocked in either direction. Removed comments still qualify, as
/// tombstones.
macro_rules! visible_comment {
    () => {
        "NOT EXISTS (SELECT 1 FROM hidden_comments h
                     WHERE h.user_id = $1 AND h.comment_id = c.id)
         AND NOT EXISTS (SELECT 1 FROM reports r
                         WHERE r.reporter_id = $1 AND r.comment_id = c.id)
         AND NOT EXISTS (SELECT 1 FROM blocks b
                         WHERE (b.blocker_id = $1 AND b.blocked_id = c.author_id)
                            OR (b.blocker_id = c.author_id AND b.blocked_id = $1))"
    };
}

pub(crate) use visible_comment;

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

/// The viewer's membership in a thread whose comments they may read and write now: 404 when the
/// privacy gate fails (unknown thread, gone host, no mutual follow, a block), then 403
/// SPOILER_LOCKED when the spoiler gate does, with this session's discussion grants.
pub(crate) async fn unlocked(
    conn: &mut PgConnection,
    key: &SessionKey,
    discussion_id: Uuid,
) -> Result<DiscussionMember, ApiError> {
    let viewer = key.user_id();
    let (discussion, member) = relationships::discussion_member(conn, viewer, discussion_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let episode_id: Uuid = sqlx::query_scalar("SELECT episode_id FROM discussions WHERE id = $1")
        .bind(discussion_id)
        .fetch_one(&mut *conn)
        .await?;
    let catalog = EpisodeCatalog::load(conn, viewer, episode_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let stored = grants::live(conn, key, RevealScope::Discussion, &[discussion_id]).await?;
    catalog
        .gate(key, &member, discussion, &stored, Utc::now())
        .unlocked()
        .map_err(|_| ApiError::new(ErrorCode::SpoilerLocked))?;
    Ok(member)
}

/// Share-locks the viewer's and the thread host's user rows in ID order for the rest of the
/// transaction. Follows, blocks, privacy changes and disabling take conflicting locks on the
/// same rows ([`crate::modules::social::follows::lock_pair`]), so a comment write's gate check
/// and its write cannot straddle a revocation.
pub(crate) async fn lock_members(
    conn: &mut PgConnection,
    viewer: Uuid,
    discussion_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT 1 FROM users
         WHERE id = $1 OR id = (SELECT host_id FROM discussions WHERE id = $2)
         ORDER BY id FOR SHARE",
    )
    .bind(viewer)
    .bind(discussion_id)
    .execute(conn)
    .await
    .map(drop)
}
