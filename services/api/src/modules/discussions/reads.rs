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
use sqlx::PgConnection;
use uuid::Uuid;

use super::{Page, PageParams, path_id, snapshot, unlocked, visible_comment};
use crate::{
    error::ApiError,
    middleware::Security,
    modules::{
        auth::session::{Role, VerifiedUser},
        policy::grants::SessionKey,
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
pub(super) struct Comment {
    id: Uuid,
    author: Option<Person>,
    body: Option<String>,
    created_at: String,
    removed: bool,
    can_delete: bool,
    can_moderate: bool,
}

impl Comment {
    /// The DTO of `row` for a viewer who may delete their own comments, and every comment when
    /// they host the thread or are an operator.
    pub(super) fn new(
        row: CommentRow,
        author: Option<Person>,
        own: bool,
        moderates_thread: bool,
        operator: bool,
    ) -> Self {
        let (id, created_at, _, body, removed) = row;
        Self {
            id,
            author,
            body: body.filter(|_| !removed),
            created_at: created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            removed,
            can_delete: !removed && (own || moderates_thread || operator),
            can_moderate: !removed && operator,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommentsPage {
    items: Vec<Comment>,
    next_cursor: Option<String>,
}

/// id, createdAt, author, body, removed.
pub(super) type CommentRow = (Uuid, DateTime<Utc>, Option<Uuid>, Option<String>, bool);

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
    let member = unlocked(&mut tx, &key, discussion_id).await?;

    let query = sqlx::query_as(concat!(
        "SELECT c.id, c.created_at, c.author_id, c.body, c.removed_at IS NOT NULL
         FROM comments c
         WHERE c.discussion_id = $2 AND ",
        visible_comment!(),
        " AND ($3::timestamptz IS NULL OR (c.created_at, c.id) > ($3, $4))
         ORDER BY c.created_at, c.id
         LIMIT $5"
    ))
    .bind(viewer)
    .bind(discussion_id);
    let mut rows: Vec<CommentRow> = page.bind(query).fetch_all(&mut *tx).await?;
    let next_cursor = page.split(&mut rows, |row| (row.1, row.0));

    let mut author_ids: Vec<Uuid> = rows.iter().filter_map(|row| row.2).collect();
    author_ids.sort_unstable();
    author_ids.dedup();
    let authors = authors(&mut tx, viewer, &author_ids).await?;
    tx.commit().await?;

    let items = rows
        .into_iter()
        .map(|row| {
            let author = row.2.and_then(|author| authors.get(&author).cloned());
            let own = row.2 == Some(viewer);
            Comment::new(row, author, own, member.is_host(), operator)
        })
        .collect();
    Ok(Json(CommentsPage { items, next_cursor }))
}

/// The cards of `ids` as `viewer` sees them, keyed by ID. A disabled (deleting) account keeps its
/// comments until erasure, but no identity.
pub(super) async fn authors(
    conn: &mut PgConnection,
    viewer: Uuid,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Person>, sqlx::Error> {
    let rows: Vec<PersonRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        " FROM users u
          WHERE u.id = ANY($2) AND u.verified_at IS NOT NULL AND u.disabled_at IS NULL"
    ))
    .bind(viewer)
    .bind(ids)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| person(viewer, row))
        .map(|person| (person.id, person))
        .collect())
}
