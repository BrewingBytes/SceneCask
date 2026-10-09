//! Library reads: LibraryItem projections, saved counts and the saved_at,id keyset page.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use super::dto::{Counts, LibraryItem, Status, poster_url};
use crate::domain::{
    progress::{self, ShowStatus},
    release::Schedule,
};

#[derive(sqlx::FromRow)]
struct ShowRow {
    id: Uuid,
    title: String,
    first_air_year: Option<i16>,
    poster_path: Option<String>,
    status: String,
    complete_import: bool,
    saved: Option<bool>,
    entry_status: Option<String>,
    revision: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct EpisodeRow {
    show_id: Uuid,
    id: Uuid,
    season: i32,
    number: i32,
    air_date: Option<NaiveDate>,
    release_timezone: Option<String>,
    starts_at: Option<DateTime<Utc>>,
    archived: bool,
    watched: bool,
}

impl EpisodeRow {
    fn schedule(&self) -> Schedule<'_> {
        match (
            self.air_date,
            self.release_timezone.as_deref(),
            self.starts_at,
        ) {
            (None, _, _) => Schedule::Undated,
            (Some(date), Some(zone), Some(starts_at)) => Schedule::Zoned {
                date,
                zone,
                starts_at,
            },
            (Some(date), _, _) => Schedule::Date(date),
        }
    }
}

fn show_status(value: &str) -> ShowStatus {
    match value {
        "returning" => ShowStatus::Returning,
        "ended" => ShowStatus::Ended,
        "canceled" => ShowStatus::Canceled,
        _ => ShowStatus::Unknown,
    }
}

/// LibraryItems for `show_ids`, in that order, with progress derived at `now`. Unknown show IDs
/// are skipped; shows without an entry read as unsaved at revision 0.
pub async fn items(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<Vec<LibraryItem>, sqlx::Error> {
    if show_ids.is_empty() {
        return Ok(Vec::new());
    }
    let shows: Vec<ShowRow> = sqlx::query_as(
        "SELECT s.id, s.title, s.first_air_year, s.poster_path, s.status, s.complete_import,
                le.saved, le.status AS entry_status, le.revision
         FROM shows s
         LEFT JOIN library_entries le ON le.show_id = s.id AND le.user_id = $1
         WHERE s.id = ANY($2)",
    )
    .bind(user_id)
    .bind(show_ids)
    .fetch_all(&mut *conn)
    .await?;
    // An episode of an archived season is archived with it; check both so a stale row can never
    // re-enter active progress.
    let rows: Vec<EpisodeRow> = sqlx::query_as(
        "SELECT e.show_id, e.id, se.number AS season, e.number, e.air_date, e.release_timezone,
                e.air_date::timestamp AT TIME ZONE e.release_timezone AS starts_at,
                (e.archived_at IS NOT NULL OR se.archived_at IS NOT NULL) AS archived,
                coalesce(p.watched, false) AS watched
         FROM episodes e
         JOIN seasons se ON se.id = e.season_id
         LEFT JOIN episode_progress p ON p.episode_id = e.id AND p.user_id = $1
         WHERE e.show_id = ANY($2)",
    )
    .bind(user_id)
    .bind(show_ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut episodes: HashMap<Uuid, Vec<&EpisodeRow>> = HashMap::new();
    for row in &rows {
        episodes.entry(row.show_id).or_default().push(row);
    }
    let mut by_id: HashMap<Uuid, LibraryItem> = shows
        .into_iter()
        .map(|show| {
            let catalog: Vec<progress::Episode> = episodes
                .get(&show.id)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|row| progress::Episode {
                    id: row.id,
                    season: row.season,
                    number: row.number,
                    schedule: row.schedule(),
                    archived: row.archived,
                    watched: row.watched,
                })
                .collect();
            let progress = progress::progress(
                &progress::Show {
                    status: show_status(&show.status),
                    complete_import: show.complete_import,
                    episodes: &catalog,
                },
                now,
            );
            let item = LibraryItem {
                show_id: show.id,
                poster_url: poster_url(show.poster_path.as_deref()),
                title: show.title,
                year: show.first_air_year,
                saved: show.saved.unwrap_or(false),
                status: show
                    .entry_status
                    .as_deref()
                    .and_then(Status::parse)
                    .unwrap_or(Status::PlanToWatch),
                revision: show.revision.unwrap_or(0),
                progress: progress.into(),
            };
            (show.id, item)
        })
        .collect();
    Ok(show_ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Counts across every saved entry, ignoring list filters.
pub async fn counts(conn: &mut PgConnection, user_id: Uuid) -> Result<Counts, sqlx::Error> {
    let (all, watching, plan_to_watch, on_hold, dropped) = sqlx::query_as(
        "SELECT count(*),
                count(*) FILTER (WHERE status = 'watching'),
                count(*) FILTER (WHERE status = 'plan_to_watch'),
                count(*) FILTER (WHERE status = 'on_hold'),
                count(*) FILTER (WHERE status = 'dropped')
         FROM library_entries WHERE user_id = $1 AND saved",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await?;
    Ok(Counts {
        all,
        watching,
        plan_to_watch,
        on_hold,
        dropped,
    })
}

/// Position of a saved entry in saved_at,id descending order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub saved_at: DateTime<Utc>,
    pub show_id: Uuid,
}

pub struct PageQuery<'a> {
    pub status: Option<Status>,
    /// Trimmed, non-empty title search.
    pub q: Option<&'a str>,
    pub after: Option<Position>,
    pub limit: i64,
}

/// Saved entries matching `query` after its position, up to `limit + 1` so the caller can tell
/// whether another page exists.
pub async fn page(
    conn: &mut PgConnection,
    user_id: Uuid,
    query: &PageQuery<'_>,
) -> Result<Vec<Position>, sqlx::Error> {
    let pattern = query.q.map(|q| format!("%{}%", escape_like(q)));
    let rows: Vec<(DateTime<Utc>, Uuid)> = sqlx::query_as(
        "SELECT le.saved_at, le.show_id
         FROM library_entries le
         JOIN shows s ON s.id = le.show_id
         WHERE le.user_id = $1 AND le.saved
           AND ($2::text IS NULL OR le.status = $2)
           AND ($3::text IS NULL OR s.title ILIKE $3 ESCAPE '\\')
           AND ($4::timestamptz IS NULL OR (le.saved_at, le.show_id) < ($4, $5))
         ORDER BY le.saved_at DESC, le.show_id DESC
         LIMIT $6",
    )
    .bind(user_id)
    .bind(query.status.map(Status::as_str))
    .bind(pattern)
    .bind(query.after.map(|position| position.saved_at))
    .bind(query.after.map(|position| position.show_id))
    .bind(query.limit + 1)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(saved_at, show_id)| Position { saved_at, show_id })
        .collect())
}

/// Saved shows with manual status Watching, in saved_at,id descending order (the Home source),
/// and whether any show is saved at all, whatever its status.
pub async fn saved_watching(
    conn: &mut PgConnection,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(Vec<LibraryItem>, bool), sqlx::Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT show_id FROM library_entries
         WHERE user_id = $1 AND saved AND status = 'watching'
         ORDER BY saved_at DESC, show_id DESC",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let any_saved = !ids.is_empty()
        || sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM library_entries WHERE user_id = $1 AND saved)",
        )
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok((items(conn, user_id, &ids, now).await?, any_saved))
}

fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_match_literally() {
        assert_eq!(escape_like(r"50%_off\"), r"50\%\_off\\");
        assert_eq!(escape_like("Hollow"), "Hollow");
    }
}
