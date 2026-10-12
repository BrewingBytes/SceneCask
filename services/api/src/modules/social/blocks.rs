//! R26 blocks (C06): `GET /me/blocks`, `PUT /me/blocks/{id}` and `DELETE /me/blocks/{id}`. A
//! block in either direction conceals both people from each other everywhere except the blocker's
//! own block list. Blocking is one transaction under [`lock_pair`]: it adds the block, removes
//! the follow edges and requests both ways (their request notifications cascade with them) and
//! revokes the pair's discussion reveal grants, so access ends before the response. Unblocking
//! removes only the block; follows stay gone.
//!
//! Blocking protects the caller, so it needs no handle, and it succeeds whether or not the other
//! person already blocks the caller: the answer never reveals the reverse block.

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::StatusCode,
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::{
    follows::{
        EdgePosition, PageParams, decode_edge_cursor, lock_pair, revoke_discussion_grants,
        split_edges,
    },
    people::PersonId,
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::{auth::session::VerifiedUser, library::page_limit},
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/me/blocks", get(list))
        .route(
            "/me/blocks/{id}",
            put(set_block::<true>).delete(set_block::<false>),
        )
        .with_state(security)
}

const DEFAULT_LIMIT: i64 = 20;

/// The block-management entry: enough to recognize and unblock someone, no relationship.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlockedPerson {
    id: Uuid,
    handle: String,
    display_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlocksPage {
    items: Vec<BlockedPerson>,
    next_cursor: Option<String>,
}

/// People the caller blocks, by block createdAt,id descending. Disabled or handleless accounts
/// are left out, like everywhere else people are listed.
async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<BlocksPage>, ApiError> {
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = decode_edge_cursor(params.cursor.as_deref())?;
    let rows: Vec<(DateTime<Utc>, Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT b.created_at, u.id, u.handle, u.display_name
         FROM blocks b
         JOIN users u ON u.id = b.blocked_id
         WHERE b.blocker_id = $1
           AND u.handle IS NOT NULL AND u.verified_at IS NOT NULL AND u.disabled_at IS NULL
           AND ($2::timestamptz IS NULL OR (b.created_at, b.blocked_id) < ($2, $3))
         ORDER BY b.created_at DESC, b.blocked_id DESC
         LIMIT $4",
    )
    .bind(current.user.id)
    .bind(after.map(|(created_at, _)| created_at))
    .bind(after.map(|(_, id)| id))
    .bind(limit + 1)
    .fetch_all(&security.pool)
    .await?;
    let positions: Vec<EdgePosition> = rows.iter().map(|row| (row.0, row.1)).collect();
    let (page, next_cursor) = split_edges(positions, limit);
    let items = rows
        .into_iter()
        .take(page.len())
        .map(|(_, id, handle, display_name)| BlockedPerson {
            id,
            display_name: display_name.unwrap_or_else(|| handle.clone()),
            handle,
        })
        .collect();
    Ok(Json(BlocksPage { items, next_cursor }))
}

/// `PUT /me/blocks/{id}` (`BLOCK = true`) blocks and `DELETE /me/blocks/{id}` unblocks. Both are
/// idempotent. Unblocking removes only the caller's own block: a reverse block stays, and no
/// follow comes back.
async fn set_block<const BLOCK: bool>(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    PersonId(target): PersonId,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<StatusCode, ApiError> {
    let viewer = current.user.id;
    if BLOCK && target == viewer {
        return Err(
            ApiError::new(ErrorCode::ValidationError).with_field("id", "You can't block yourself.")
        );
    }
    let mut tx = security.pool.begin().await?;
    lock_pair(&mut tx, viewer, target).await?;
    if BLOCK {
        add(&mut tx, viewer, target).await?;
    } else {
        sqlx::query("DELETE FROM blocks WHERE blocker_id = $1 AND blocked_id = $2")
            .bind(viewer)
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Blocks `target` and removes everything the pair shared, under the caller's [`lock_pair`].
async fn add(conn: &mut PgConnection, viewer: Uuid, target: Uuid) -> Result<(), ApiError> {
    // Any listed person can be blocked, including someone who already blocks the caller.
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users
                        WHERE id = $1 AND handle IS NOT NULL AND verified_at IS NOT NULL
                          AND disabled_at IS NULL)",
    )
    .bind(target)
    .fetch_one(&mut *conn)
    .await?;
    if !known {
        return Err(ApiError::not_found());
    }
    sqlx::query(
        "INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(viewer)
    .bind(target)
    .execute(&mut *conn)
    .await?;
    // Deleting the edges cascades to their follow request notifications.
    sqlx::query(
        "DELETE FROM follows
         WHERE (follower_id = $1 AND followee_id = $2) OR (follower_id = $2 AND followee_id = $1)",
    )
    .bind(viewer)
    .bind(target)
    .execute(&mut *conn)
    .await?;
    revoke_discussion_grants(conn, viewer, target).await?;
    Ok(())
}
