//! R30 hosted threads (C07): `GET /episodes/{id}/discussions` and `POST /episodes/{id}/discussions`.
//! Each user hosts at most one thread per episode. The list holds only threads the viewer may
//! access now (their own, or a current mutual approved follow of the host with no block), by
//! createdAt,id descending. Items carry the host card and the spoiler state only, never comments,
//! previews, counts or other participants. Creating needs no progress; reading stays gated.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    routing::get,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::{Page, PageParams, path_id, snapshot};
use crate::{
    domain::{
        access::{self, DiscussionMember, Relationship, Viewer},
        progress::Episode,
        release::Schedule,
        spoilers::{
            self, Access, Discussion, DiscussionGate, DiscussionSummary, Grant, RevealScope,
        },
    },
    error::ApiError,
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::{
        auth::session::VerifiedUser,
        policy::grants::{self, SessionKey, StoredGrant},
        social::people::{Person, listed, person, person_columns},
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/episodes/{id}/discussions", get(list).post(create))
        .with_state(security)
}

/// One catalog episode with the viewer's mark: id, season, number, air date, release timezone,
/// start instant, archived, watched.
type EpisodeRow = (
    Uuid,
    i32,
    i32,
    Option<NaiveDate>,
    Option<String>,
    Option<DateTime<Utc>>,
    bool,
    bool,
);

/// A discussion target's whole show joined with the viewer's own marks (never the host's), which
/// the spoiler gate needs to find unmarked predecessors.
pub(super) struct EpisodeCatalog {
    target: usize,
    rows: Vec<EpisodeRow>,
}

impl EpisodeCatalog {
    /// `None` when the episode is unknown. Archived episodes still load; the gate skips archived
    /// predecessors.
    pub(super) async fn load(
        conn: &mut PgConnection,
        viewer: Uuid,
        episode_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        let rows: Vec<EpisodeRow> = sqlx::query_as(
            "SELECT e.id, se.number AS season, e.number, e.air_date, e.release_timezone,
                    e.air_date::timestamp AT TIME ZONE e.release_timezone AS starts_at,
                    (e.archived_at IS NOT NULL OR se.archived_at IS NOT NULL) AS archived,
                    coalesce(p.watched, false) AS watched
             FROM episodes t
             JOIN episodes e ON e.show_id = t.show_id
             JOIN seasons se ON se.id = e.season_id
             LEFT JOIN episode_progress p ON p.episode_id = e.id AND p.user_id = $1
             WHERE t.id = $2",
        )
        .bind(viewer)
        .bind(episode_id)
        .fetch_all(conn)
        .await?;
        Ok(rows
            .iter()
            .position(|row| row.0 == episode_id)
            .map(|target| Self { target, rows }))
    }

    /// The spoiler decision for `discussion` on this target, for `member` under this session.
    pub(super) fn gate(
        &self,
        key: &SessionKey,
        member: &DiscussionMember,
        discussion: Discussion,
        grants: &[StoredGrant],
        now: DateTime<Utc>,
    ) -> DiscussionGate {
        let episodes: Vec<Episode> = self
            .rows
            .iter()
            .map(|row| Episode {
                id: row.0,
                season: row.1,
                number: row.2,
                schedule: Schedule::from_catalog(row.3, row.4.as_deref(), row.5),
                archived: row.6,
                watched: row.7,
            })
            .collect();
        let grants: Vec<Grant> = grants.iter().map(StoredGrant::grant).collect();
        spoilers::discussion_gate(
            member,
            key.session(),
            discussion,
            &episodes[self.target],
            &episodes,
            &grants,
            now,
        )
    }
}

/// The viewer's membership in `host`'s threads, from the host card's current edges. A card exists
/// only without a block either way, so the edges alone decide.
fn membership(viewer: Uuid, host: &Person) -> Option<DiscussionMember> {
    let relationship = Relationship {
        outgoing: host.relationship.outgoing,
        incoming: host.relationship.incoming,
        ..Relationship::default()
    };
    access::discussion_member(Viewer::Member(viewer), host.id, &relationship)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscussionsPage {
    items: Vec<DiscussionSummary<Person>>,
    next_cursor: Option<String>,
}

/// A thread with its host's card columns (see [`person_columns`]).
type ThreadRow = (
    Uuid,
    DateTime<Utc>,
    Uuid,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    bool,
    bool,
);

async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<DiscussionsPage>, ApiError> {
    let episode_id = path_id(&id)?;
    let page = Page::parse(params)?;
    let key = SessionKey::of(&current);
    let viewer = key.user_id();
    let now = Utc::now();
    let mut tx = snapshot(&security.pool).await?;
    let catalog = EpisodeCatalog::load(&mut tx, viewer, episode_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let query = sqlx::query_as(concat!(
        "SELECT d.id, d.created_at, ",
        person_columns!(),
        " FROM discussions d
          JOIN users u ON u.id = d.host_id
          WHERE d.episode_id = $2 AND ",
        listed!(),
        " AND (u.id = $1
               OR (EXISTS (SELECT 1 FROM follows WHERE follower_id = $1 AND followee_id = u.id
                                                   AND state = 'approved')
                   AND EXISTS (SELECT 1 FROM follows WHERE follower_id = u.id AND followee_id = $1
                                                       AND state = 'approved')))
            AND ($3::timestamptz IS NULL OR (d.created_at, d.id) < ($3, $4))
          ORDER BY d.created_at DESC, d.id DESC
          LIMIT $5"
    ))
    .bind(viewer)
    .bind(episode_id);
    let mut rows: Vec<ThreadRow> = page.bind(query).fetch_all(&mut *tx).await?;
    let next_cursor = page.split(&mut rows, |row| (row.1, row.0));
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let stored = grants::live(&mut tx, &key, RevealScope::Discussion, &ids).await?;
    tx.commit().await?;

    let items = rows
        .into_iter()
        .filter_map(
            |(id, _, host_id, handle, name, visibility, out, inc, blocks, blocked)| {
                let host = person(
                    viewer,
                    (host_id, handle, name, visibility, out, inc, blocks, blocked),
                )?;
                let member = membership(viewer, &host)?;
                let discussion = Discussion { id, host_id };
                Some(
                    catalog
                        .gate(&key, &member, discussion, &stored, now)
                        .summary(host),
                )
            },
        )
        .collect();
    Ok(Json(DiscussionsPage { items, next_cursor }))
}

#[derive(Debug, Serialize)]
struct DiscussionResult {
    id: Uuid,
    access: Access,
}

/// 201 with the new caller-hosted thread, or 200 with the existing one. The unique host+episode
/// key makes concurrent creates converge on one thread.
async fn create(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<(StatusCode, Json<DiscussionResult>), ApiError> {
    let episode_id = path_id(&id)?;
    let key = SessionKey::of(&current);
    let host = key.user_id();
    let mut conn = security.pool.acquire().await?;
    let catalog = EpisodeCatalog::load(&mut conn, host, episode_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let created: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO discussions (host_id, episode_id) VALUES ($1, $2)
         ON CONFLICT (host_id, episode_id) DO NOTHING
         RETURNING id",
    )
    .bind(host)
    .bind(episode_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (status, id) = match created {
        Some(id) => (StatusCode::CREATED, id),
        None => {
            // A new statement sees the conflicting thread once its insert committed.
            let id = sqlx::query_scalar(
                "SELECT id FROM discussions WHERE host_id = $1 AND episode_id = $2",
            )
            .bind(host)
            .bind(episode_id)
            .fetch_one(&mut *conn)
            .await?;
            (StatusCode::OK, id)
        }
    };
    let member = access::discussion_member(Viewer::Member(host), host, &Relationship::default())
        .ok_or_else(ApiError::not_found)?;
    let stored = grants::live(&mut conn, &key, RevealScope::Discussion, &[id]).await?;
    let gate = catalog.gate(
        &key,
        &member,
        Discussion { id, host_id: host },
        &stored,
        Utc::now(),
    );
    Ok((
        status,
        Json(DiscussionResult {
            id,
            access: gate.access(),
        }),
    ))
}
