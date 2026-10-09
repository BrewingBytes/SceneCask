//! Viewer-filtered catalog reads: the Show DTO and Episode DTOs joined with the viewer's own
//! progress and this session's reveal grants. Protected episode fields are wrapped as soon as
//! they leave the database and only the spoiler policy can unwrap them.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

use crate::{
    domain::{
        release::Schedule,
        spoilers::{self, EpisodeDetails, EpisodeFacts, EpisodeView, Grant, RevealScope},
    },
    modules::{
        catalog::refresh::metadata_stale,
        library::{
            dto::{LibraryItem, poster_url, still_url},
            repository::items,
        },
        policy::grants::{self, SessionKey},
    },
};

#[derive(Debug, Serialize)]
pub struct SeasonCount {
    pub number: i32,
    pub count: i64,
}

/// C05 Show DTO. The synopsis is the series synopsis; nothing here is episode-level.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowView {
    id: Uuid,
    title: String,
    year: Option<i16>,
    genres: Vec<String>,
    synopsis: String,
    poster_url: Option<String>,
    status: String,
    catalog_revision: i64,
    metadata_stale: bool,
    release_info_incomplete: bool,
    seasons: Vec<SeasonCount>,
    library: Option<LibraryItem>,
    tracking_revision: i64,
}

/// The show as the viewer sees it, or `None` when it is unknown.
pub async fn show(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<ShowView>, sqlx::Error> {
    type ShowTuple = (
        String,
        Option<i16>,
        Json<Vec<String>>,
        Option<String>,
        Option<String>,
        String,
        i64,
        DateTime<Utc>,
        bool,
        i64,
    );
    let row: Option<ShowTuple> = sqlx::query_as(
        "SELECT s.title, s.first_air_year, s.genres, s.synopsis, s.poster_path, s.status,
                s.catalog_revision, s.fetched_at,
                EXISTS (SELECT 1 FROM library_entries le
                        WHERE le.user_id = $1 AND le.show_id = s.id) AS has_entry,
                coalesce((SELECT t.revision FROM tracking_show_state t
                          WHERE t.user_id = $1 AND t.show_id = s.id), 0) AS tracking_revision
         FROM shows s WHERE s.id = $2",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((
        title,
        year,
        Json(genres),
        synopsis,
        poster_path,
        status,
        catalog_revision,
        fetched_at,
        has_entry,
        tracking_revision,
    )) = row
    else {
        return Ok(None);
    };
    let seasons: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT se.number, count(e.id) FILTER (WHERE e.archived_at IS NULL)
         FROM seasons se
         LEFT JOIN episodes e ON e.season_id = se.id
         WHERE se.show_id = $1 AND se.archived_at IS NULL
         GROUP BY se.number
         ORDER BY se.number",
    )
    .bind(show_id)
    .fetch_all(&mut *conn)
    .await?;
    // The LibraryItem projection derives progress; release completeness is a catalog fact it
    // already computes for the show.
    let item = items(conn, user_id, &[show_id], now).await?.pop();
    let release_info_incomplete = item
        .as_ref()
        .is_some_and(|item| item.progress.release_info_incomplete);
    Ok(Some(ShowView {
        id: show_id,
        title,
        year,
        genres,
        synopsis: synopsis.unwrap_or_default(),
        poster_url: poster_url(poster_path.as_deref()),
        status,
        catalog_revision,
        metadata_stale: metadata_stale(fetched_at, now),
        release_info_incomplete,
        seasons: seasons
            .into_iter()
            .map(|(number, count)| SeasonCount { number, count })
            .collect(),
        library: item.filter(|_| has_entry),
        tracking_revision,
    }))
}

pub async fn show_exists(conn: &mut PgConnection, show_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM shows WHERE id = $1)")
        .bind(show_id)
        .fetch_one(conn)
        .await
}

type EpisodeTuple = (
    Uuid,
    Uuid,
    i32,
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<NaiveDate>,
    Option<String>,
    Option<DateTime<Utc>>,
    bool,
    i64,
);

/// One episode row with the viewer's progress; `$1` is the viewer.
macro_rules! episode_select {
    () => {
        "SELECT e.id, e.show_id, se.number AS season, e.number, e.title, e.overview, e.still_path,
                e.air_date, e.release_timezone,
                e.air_date::timestamp AT TIME ZONE e.release_timezone AS starts_at,
                coalesce(p.watched, false), coalesce(p.revision, 0)
         FROM episodes e
         JOIN seasons se ON se.id = e.season_id
         LEFT JOIN episode_progress p ON p.episode_id = e.id AND p.user_id = $1"
    };
}

/// Position in season number, episode number, id ascending order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpisodePosition {
    pub season: i32,
    pub number: i32,
    pub id: Uuid,
}

pub struct EpisodeQuery {
    /// `None` lists regular seasons only; specials need an explicit `Some(0)`.
    pub season: Option<i32>,
    pub after: Option<EpisodePosition>,
    pub limit: i64,
}

/// A page of active episodes of `show_id` matching `query`, and the position to continue after
/// when another page exists.
pub async fn episodes(
    conn: &mut PgConnection,
    key: &SessionKey,
    show_id: Uuid,
    query: &EpisodeQuery,
    now: DateTime<Utc>,
) -> Result<(Vec<EpisodeView>, Option<EpisodePosition>), sqlx::Error> {
    let mut rows: Vec<EpisodeTuple> = sqlx::query_as(concat!(
        episode_select!(),
        " WHERE e.show_id = $2 AND e.archived_at IS NULL AND se.archived_at IS NULL
            AND CASE WHEN $3::int IS NULL THEN se.number > 0 ELSE se.number = $3 END
            AND ($4::int IS NULL OR (se.number, e.number, e.id) > ($4, $5, $6))
          ORDER BY se.number, e.number, e.id
          LIMIT $7"
    ))
    .bind(key.user_id())
    .bind(show_id)
    .bind(query.season)
    .bind(query.after.map(|position| position.season))
    .bind(query.after.map(|position| position.number))
    .bind(query.after.map(|position| position.id))
    .bind(query.limit + 1)
    .fetch_all(&mut *conn)
    .await?;
    let next = if rows.len() as i64 > query.limit {
        rows.truncate(query.limit as usize);
        rows.last().map(|row| EpisodePosition {
            season: row.2,
            number: row.3,
            id: row.0,
        })
    } else {
        None
    };
    Ok((project(conn, key, rows, now).await?, next))
}

/// One episode, archived or not, or `None` when it is unknown.
pub async fn episode(
    conn: &mut PgConnection,
    key: &SessionKey,
    episode_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<EpisodeView>, sqlx::Error> {
    let rows: Vec<EpisodeTuple> = sqlx::query_as(concat!(episode_select!(), " WHERE e.id = $2"))
        .bind(key.user_id())
        .bind(episode_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(project(conn, key, rows, now).await?.pop())
}

/// Gates each row through the spoiler policy with this session's episode grants.
async fn project(
    conn: &mut PgConnection,
    key: &SessionKey,
    rows: Vec<EpisodeTuple>,
    now: DateTime<Utc>,
) -> Result<Vec<EpisodeView>, sqlx::Error> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let stored = grants::live(conn, key, RevealScope::EpisodeDetails, &ids).await?;
    let grants: Vec<Grant> = stored.iter().map(grants::StoredGrant::grant).collect();
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                show_id,
                season,
                number,
                title,
                overview,
                still_path,
                air_date,
                release_timezone,
                starts_at,
                watched,
                revision,
            )| {
                let schedule =
                    Schedule::from_catalog(air_date, release_timezone.as_deref(), starts_at);
                let facts = EpisodeFacts {
                    id,
                    show_id,
                    season,
                    number,
                    release: schedule.release(now),
                    watched,
                    revision,
                };
                let details =
                    EpisodeDetails::protect(title, overview, still_url(still_path.as_deref()));
                spoilers::episode_view(key.session(), facts, details, &grants, now)
            },
        )
        .collect())
}
