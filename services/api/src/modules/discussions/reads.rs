//! R30 gated comment reads (C07): `GET /discussions/{id}/comments`, by createdAt,id ascending.
//! Privacy runs first: a thread the viewer may not access now is 404, even with a reveal grant.
//! The spoiler gate runs second: a locked thread is 403 SPOILER_LOCKED with no thread content.
//! Comments by someone blocked in either direction, and comments the viewer hid or reported, are
//! omitted with their authors' identities. Removed comments are bodyless tombstones.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{Page, PageParams, path_id, snapshot, threads::EpisodeCatalog};
use crate::{
    domain::spoilers::RevealScope,
    error::{ApiError, ErrorCode},
    middleware::Security,
    modules::{
        auth::session::{Role, VerifiedUser},
        policy::{
            grants::{self, SessionKey},
            relationships,
        },
        social::people::{Person, PersonRow, person, person_columns},
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/discussions/{id}/comments", get(list))
        .with_state(security)
}

/// API `Comment`. A tombstone keeps its position but never its original body.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Comment {
    id: Uuid,
    author: Option<Person>,
    body: Option<String>,
    created_at: String,
    removed: bool,
    can_delete: bool,
    can_moderate: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommentsPage {
    items: Vec<Comment>,
    next_cursor: Option<String>,
}

/// id, createdAt, author, body, removed.
type CommentRow = (Uuid, DateTime<Utc>, Option<Uuid>, Option<String>, bool);

async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<CommentsPage>, ApiError> {
    let discussion_id = path_id(&id)?;
    let page = Page::parse(params)?;
    let key = SessionKey::of(&current);
    let viewer = key.user_id();
    let operator = current.user.role == Role::Operator;
    let mut tx = snapshot(&security.pool).await?;
    let (discussion, member) = relationships::discussion_member(&mut tx, viewer, discussion_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let episode_id: Uuid = sqlx::query_scalar("SELECT episode_id FROM discussions WHERE id = $1")
        .bind(discussion_id)
        .fetch_one(&mut *tx)
        .await?;
    let catalog = EpisodeCatalog::load(&mut tx, viewer, episode_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let stored = grants::live(&mut tx, &key, RevealScope::Discussion, &[discussion_id]).await?;
    catalog
        .gate(&key, &member, discussion, &stored, Utc::now())
        .unlocked()
        .map_err(|_| ApiError::new(ErrorCode::SpoilerLocked))?;

    let query = sqlx::query_as(
        "SELECT c.id, c.created_at, c.author_id, c.body, c.removed_at IS NOT NULL
         FROM comments c
         WHERE c.discussion_id = $2
           AND NOT EXISTS (SELECT 1 FROM hidden_comments h
                           WHERE h.user_id = $1 AND h.comment_id = c.id)
           AND NOT EXISTS (SELECT 1 FROM reports r
                           WHERE r.reporter_id = $1 AND r.comment_id = c.id)
           AND NOT EXISTS (SELECT 1 FROM blocks b
                           WHERE (b.blocker_id = $1 AND b.blocked_id = c.author_id)
                              OR (b.blocker_id = c.author_id AND b.blocked_id = $1))
           AND ($3::timestamptz IS NULL OR (c.created_at, c.id) > ($3, $4))
         ORDER BY c.created_at, c.id
         LIMIT $5",
    )
    .bind(viewer)
    .bind(discussion_id);
    let mut rows: Vec<CommentRow> = page.bind(query).fetch_all(&mut *tx).await?;
    let next_cursor = page.split(&mut rows, |row| (row.1, row.0));

    let mut author_ids: Vec<Uuid> = rows.iter().filter_map(|row| row.2).collect();
    author_ids.sort_unstable();
    author_ids.dedup();
    // A disabled (deleting) account keeps its comments until erasure, but no identity.
    let authors: Vec<PersonRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        " FROM users u
          WHERE u.id = ANY($2) AND u.verified_at IS NOT NULL AND u.disabled_at IS NULL"
    ))
    .bind(viewer)
    .bind(&author_ids)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let authors: HashMap<Uuid, Person> = authors
        .into_iter()
        .filter_map(|row| person(viewer, row))
        .map(|person| (person.id, person))
        .collect();

    let items = rows
        .into_iter()
        .map(|(id, created_at, author_id, body, removed)| {
            let own = author_id == Some(viewer);
            Comment {
                id,
                author: author_id.and_then(|author| authors.get(&author).cloned()),
                body: body.filter(|_| !removed),
                created_at: created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
                removed,
                can_delete: !removed && (own || member.is_host() || operator),
                can_moderate: !removed && operator,
            }
        })
        .collect();
    Ok(Json(CommentsPage { items, next_cursor }))
}
