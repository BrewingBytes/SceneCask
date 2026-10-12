//! R27 recommendations (C06): `GET /recommendations`. Shows saved by the people the viewer follows
//! with an approved edge, which the viewer has not saved, ranked by how many of those people saved
//! them, then by show ID. Follows, blocks and account state are joined at read time, so no friends
//! yields an empty page rather than a fake feed. The cursor pins the first page's snapshot time:
//! later saves stay out of the following pages, so a new save cannot reorder what was already
//! listed. No trending or AI signal takes part.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{
    follows::PageParams,
    people::{Person, listed, person, person_columns},
};
use crate::{
    error::ApiError,
    middleware::Security,
    modules::{
        auth::session::VerifiedUser,
        library::{decode_opaque, dto::poster_url, encode_opaque, page_limit},
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/recommendations", get(list))
        .with_state(security)
}

const DEFAULT_LIMIT: i64 = 20;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecommendedShow {
    id: Uuid,
    title: String,
    year: Option<i16>,
    poster_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct Reason {
    kind: &'static str,
    people: Vec<Person>,
}

#[derive(Debug, Serialize)]
struct Recommendation {
    show: RecommendedShow,
    reason: Reason,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecommendationsPage {
    items: Vec<Recommendation>,
    next_cursor: Option<String>,
}

/// The snapshot time and the last listed position: people count descending, show ID ascending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cursor {
    snapshot: DateTime<Utc>,
    count: i64,
    show_id: Uuid,
}

fn encode_cursor(cursor: &Cursor) -> String {
    encode_opaque(&format!(
        "{}.{}.{}",
        cursor.snapshot.timestamp_micros(),
        cursor.count,
        cursor.show_id.simple()
    ))
}

fn decode_cursor(cursor: &str) -> Result<Cursor, ApiError> {
    decode_opaque(cursor, |text| {
        let mut parts = text.split('.');
        let snapshot = DateTime::from_timestamp_micros(parts.next()?.parse().ok()?)?;
        let count = parts.next()?.parse().ok().filter(|count| *count > 0)?;
        let show_id = Uuid::try_parse(parts.next()?).ok()?;
        parts.next().is_none().then_some(Cursor {
            snapshot,
            count,
            show_id,
        })
    })
}

/// A [`PersonRow`] followed by the saved show's ID.
type SaverRow = (
    Uuid,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    bool,
    bool,
    Uuid,
);

/// Saved entries of `u`, a followed person the viewer (`$1`) may see, as of `$2`.
macro_rules! followed_saves {
    () => {
        concat!(
            " FROM library_entries l
             JOIN follows f ON f.follower_id = $1 AND f.followee_id = l.user_id
                           AND f.state = 'approved'
             JOIN users u ON u.id = l.user_id
             WHERE l.saved AND l.saved_at <= $2 AND ",
            listed!(),
            " AND NOT EXISTS (SELECT 1 FROM library_entries m
                              WHERE m.user_id = $1 AND m.show_id = l.show_id AND m.saved)"
        )
    };
}

async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<RecommendationsPage>, ApiError> {
    let viewer = current.user.id;
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = params.cursor.as_deref().map(decode_cursor).transpose()?;
    let mut tx = security.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let snapshot = match after {
        Some(after) => after.snapshot,
        None => {
            sqlx::query_scalar("SELECT now()")
                .fetch_one(&mut *tx)
                .await?
        }
    };
    let mut ranked: Vec<(Uuid, i64)> = sqlx::query_as(concat!(
        "SELECT l.show_id, count(*)",
        followed_saves!(),
        " GROUP BY l.show_id
         HAVING $3::bigint IS NULL OR (count(*), $4::uuid) < ($3, l.show_id)
         ORDER BY count(*) DESC, l.show_id
         LIMIT $5"
    ))
    .bind(viewer)
    .bind(snapshot)
    .bind(after.map(|after| after.count))
    .bind(after.map(|after| after.show_id))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    let next_cursor = (ranked.len() as i64 > limit)
        .then(|| {
            ranked.truncate(limit as usize);
            ranked.last().map(|&(show_id, count)| {
                encode_cursor(&Cursor {
                    snapshot,
                    count,
                    show_id,
                })
            })
        })
        .flatten();
    let ids: Vec<Uuid> = ranked.iter().map(|&(id, _)| id).collect();

    let shows: Vec<(Uuid, String, Option<i16>, Option<String>)> = sqlx::query_as(
        "SELECT id, title, first_air_year, poster_path FROM shows WHERE id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    let mut shows: HashMap<Uuid, RecommendedShow> = shows
        .into_iter()
        .map(|(id, title, year, poster_path)| {
            let poster_url = poster_url(poster_path.as_deref());
            (
                id,
                RecommendedShow {
                    id,
                    title,
                    year,
                    poster_url,
                },
            )
        })
        .collect();

    // The show ID follows the person columns, which bind the viewer as `$1`.
    let savers: Vec<SaverRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        ", l.show_id",
        followed_saves!(),
        " AND l.show_id = ANY($3)
         ORDER BY u.handle, u.id"
    ))
    .bind(viewer)
    .bind(snapshot)
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut people: HashMap<Uuid, Vec<Person>> = HashMap::new();
    for (id, handle, display_name, visibility, outgoing, incoming, blocks, blocked_by, show_id) in
        savers
    {
        let row = (
            id,
            handle,
            display_name,
            visibility,
            outgoing,
            incoming,
            blocks,
            blocked_by,
        );
        if let Some(person) = person(viewer, row) {
            people.entry(show_id).or_default().push(person);
        }
    }

    let items = ids
        .iter()
        .filter_map(|id| {
            Some(Recommendation {
                show: shows.remove(id)?,
                reason: Reason {
                    kind: "followed_people",
                    people: people.remove(id).unwrap_or_default(),
                },
            })
        })
        .collect();
    Ok(Json(RecommendationsPage { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_tampering() {
        let cursor = Cursor {
            snapshot: Utc.with_ymd_and_hms(2026, 10, 12, 9, 30, 0).unwrap(),
            count: 3,
            show_id: Uuid::from_u128(7),
        };
        assert_eq!(decode_cursor(&encode_cursor(&cursor)).unwrap(), cursor);
        for invalid in [
            "",
            "not base64!",
            &URL_SAFE_NO_PAD.encode("1.0.00000000000000000000000000000007"),
            &URL_SAFE_NO_PAD.encode("1.2.not-a-uuid"),
            &URL_SAFE_NO_PAD.encode("1.2.00000000000000000000000000000007.9"),
            &URL_SAFE_NO_PAD.encode("x.2.00000000000000000000000000000007"),
        ] {
            assert_eq!(
                decode_cursor(invalid).unwrap_err().status().as_u16(),
                400,
                "{invalid}"
            );
        }
    }
}
