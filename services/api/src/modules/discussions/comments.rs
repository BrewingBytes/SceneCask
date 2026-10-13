//! R31 comment writes (C07): `POST /discussions/{id}/comments` and `DELETE /comments/{id}`.
//! Posting rechecks the privacy and spoiler gates at write time under [`lock_members`], so a
//! locked or revoked thread rejects the post without writing. Bodies are trimmed plain text of
//! 1–5000 Unicode characters, stored and returned verbatim as JSON strings (clients render them
//! as text), with no edits, attachments or link previews, at most 10 per minute per user.
//!
//! Removal turns a comment into a bodyless tombstone for every reader. Only its author, the host
//! of its thread and operators may remove it; anyone else gets 404, like an unknown comment.
//! Repeating an authorized removal is a no-op.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    lock_members, path_id,
    reads::{Comment, CommentRow, authors},
    unlocked,
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
        rate_limit::COMMENT_RULES,
    },
    modules::{
        auth::session::{Role, VerifiedUser},
        policy::grants::SessionKey,
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/discussions/{id}/comments", post(create))
        .route("/comments/{id}", delete(remove))
        .with_state(security)
}

const MAX_CHARS: usize = 5000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentRequest {
    body: String,
}

/// The trimmed body, or 422 when it is empty, too long, or holds control characters other than
/// line breaks and tabs (PostgreSQL text cannot store NUL).
fn plain_text(body: &str) -> Result<&str, ApiError> {
    let body = body.trim();
    let invalid =
        |message| Err(ApiError::new(ErrorCode::ValidationError).with_field("body", message));
    if body.is_empty() || body.chars().count() > MAX_CHARS {
        return invalid("Write 1 to 5000 characters.");
    }
    if body
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return invalid("Use plain text only.");
    }
    Ok(body)
}

/// 201 with the new comment as its author sees it.
async fn create(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<CommentRequest>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    let discussion_id = path_id(&id)?;
    let body = plain_text(&request.body)?;
    // An author card needs a handle; without one the comment would read as anonymous.
    if current.user.handle.is_none() {
        return Err(ApiError::new(ErrorCode::ValidationError)
            .with_field("handle", "Choose a handle before commenting."));
    }
    let key = SessionKey::of(&current);
    let viewer = key.user_id();
    let mut tx = security.pool.begin().await?;
    lock_members(&mut tx, viewer, discussion_id).await?;
    let member = unlocked(&mut tx, &key, discussion_id).await?;
    // Counted only once the gates pass, so rejected posts do not spend the budget.
    security
        .limiter
        .check(&format!("comments.post:{viewer}"), &COMMENT_RULES)?;
    let row: CommentRow = sqlx::query_as(
        "INSERT INTO comments (discussion_id, author_id, body) VALUES ($1, $2, $3)
         RETURNING id, created_at, author_id, body, removed_at IS NOT NULL",
    )
    .bind(discussion_id)
    .bind(viewer)
    .bind(body)
    .fetch_one(&mut *tx)
    .await?;
    let author = authors(&mut tx, viewer, &[viewer]).await?.remove(&viewer);
    tx.commit().await?;
    let operator = current.user.role == Role::Operator;
    Ok((
        StatusCode::CREATED,
        Json(Comment::new(row, author, true, member.is_host(), operator)),
    ))
}

/// 204 once the comment is a tombstone. The comment row is locked first, so concurrent removals
/// record one remover and timestamp.
async fn remove(
    VerifiedUser(current): VerifiedUser,
    Path(id): Path<String>,
    State(security): State<Security>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<StatusCode, ApiError> {
    let (viewer, operator) = (current.user.id, current.user.role == Role::Operator);
    let comment_id = path_id(&id)?;
    let mut tx = security.pool.begin().await?;
    let target: Option<(Option<Uuid>, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT c.author_id, d.host_id, c.removed_at IS NOT NULL
         FROM comments c JOIN discussions d ON d.id = c.discussion_id
         WHERE c.id = $1
         FOR UPDATE OF c",
    )
    .bind(comment_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (author, host, removed) = target.ok_or_else(ApiError::not_found)?;
    if !(operator || author == Some(viewer) || host == Some(viewer)) {
        return Err(ApiError::not_found());
    }
    if !removed {
        sqlx::query(
            "UPDATE comments SET body = NULL, removed_at = now(), removed_by = $2 WHERE id = $1",
        )
        .bind(comment_id)
        .bind(viewer)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
