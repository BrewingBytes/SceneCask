//! R27 activity feed (C06): `GET /activity`. Events projected by
//! [`crate::modules::tracking::events`] from the people the viewer follows with an approved edge,
//! by createdAt,id descending. Follows, blocks and the actor's account state are joined at read
//! time, so an unfollow, follower removal or block hides old events on the next request. An
//! approved follow already grants library access whatever the actor's visibility. Items carry the
//! show card and an episode code only, never episode titles, overviews or stills.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{
    follows::{EdgePosition, PageParams, decode_edge_cursor, split_edges},
    people::{Person, PersonRow, listed, person, person_columns},
};
use crate::{
    error::ApiError,
    middleware::Security,
    modules::{
        auth::session::VerifiedUser,
        library::{dto::poster_url, page_limit},
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/activity", get(list))
        .with_state(security)
}

const DEFAULT_LIMIT: i64 = 20;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityShow {
    id: Uuid,
    title: String,
    poster_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct EpisodeCode {
    id: Uuid,
    season: i32,
    number: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityEvent {
    id: Uuid,
    actor: Person,
    kind: String,
    show: ActivityShow,
    episode: Option<EpisodeCode>,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityPage {
    items: Vec<ActivityEvent>,
    next_cursor: Option<String>,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    actor_id: Uuid,
    kind: String,
    show_id: Uuid,
    title: String,
    poster_path: Option<String>,
    episode_id: Option<Uuid>,
    season: Option<i32>,
    number: Option<i32>,
}

async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ActivityPage>, ApiError> {
    let viewer = current.user.id;
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = decode_edge_cursor(params.cursor.as_deref())?;
    // One snapshot for the events and their actors' cards, so both see the same edges.
    let mut tx = security.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let rows: Vec<EventRow> = sqlx::query_as(concat!(
        "SELECT a.id, a.created_at, a.actor_id, a.kind, s.id AS show_id, s.title, s.poster_path,
                e.id AS episode_id, se.number AS season, e.number
         FROM activity_events a
         JOIN follows f ON f.follower_id = $1 AND f.followee_id = a.actor_id
                       AND f.state = 'approved'
         JOIN users u ON u.id = a.actor_id
         JOIN shows s ON s.id = a.show_id
         LEFT JOIN episodes e ON e.id = a.episode_id
         LEFT JOIN seasons se ON se.id = e.season_id
         WHERE ",
        listed!(),
        " AND ($2::timestamptz IS NULL OR (a.created_at, a.id) < ($2, $3))
         ORDER BY a.created_at DESC, a.id DESC
         LIMIT $4"
    ))
    .bind(viewer)
    .bind(after.map(|(created_at, _)| created_at))
    .bind(after.map(|(_, id)| id))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    let positions: Vec<EdgePosition> = rows.iter().map(|row| (row.created_at, row.id)).collect();
    let (page, next_cursor) = split_edges(positions, limit);
    let rows = &rows[..page.len()];

    let mut actor_ids: Vec<Uuid> = rows.iter().map(|row| row.actor_id).collect();
    actor_ids.sort_unstable();
    actor_ids.dedup();
    let actors: Vec<PersonRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        " FROM users u WHERE u.id = ANY($2)"
    ))
    .bind(viewer)
    .bind(&actor_ids)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let actors: HashMap<Uuid, Person> = actors
        .into_iter()
        .filter_map(|row| person(viewer, row))
        .map(|person| (person.id, person))
        .collect();

    let items = rows
        .iter()
        .filter_map(|row| {
            let episode = match (row.episode_id, row.season, row.number) {
                (Some(id), Some(season), Some(number)) => Some(EpisodeCode { id, season, number }),
                _ => None,
            };
            Some(ActivityEvent {
                id: row.id,
                actor: actors.get(&row.actor_id)?.clone(),
                kind: row.kind.clone(),
                show: ActivityShow {
                    id: row.show_id,
                    title: row.title.clone(),
                    poster_url: poster_url(row.poster_path.as_deref()),
                },
                episode,
                created_at: row.created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            })
        })
        .collect();
    Ok(Json(ActivityPage { items, next_cursor }))
}
