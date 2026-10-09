//! R17 explicit catch-up (C05, C09 case 4), relative to `/api/v1`:
//! `POST /shows/{id}/catch-up/preview` and `POST /shows/{id}/catch-up`. Integration (R24/R38)
//! mounts [`routes`] behind the C03 security layers.
//!
//! A preview lists the exact released, unmarked regular episodes through the chosen endpoint and
//! every exclusion, and stores that set for five minutes bound to the catalog, tracking and
//! library revisions it was read at. Commit marks exactly that set under the shared show →
//! tracking state → library entry → episode lock order, or rejects the whole request as
//! PREVIEW_STALE when any bound revision moved: it never widens or narrows the set. Commit records
//! one action through the shared recorder, so Undo reverses only the episodes it marked.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, types::Json as SqlJson};
use uuid::Uuid;

use super::{
    mutations::{bump_tracking, lock_tracking, result, write_progress},
    path_id,
};
use crate::{
    domain::release::{ReleaseState, Schedule},
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson},
    modules::{
        auth::session::VerifiedUser,
        library::{
            actions::{self, Change, Entity},
            dto::MutationResult,
            idempotency::{self, IdempotencyKey},
            service::{LockedEntry, tracking_revision},
        },
    },
};

/// C05: a preview can be confirmed for 5 minutes.
pub const PREVIEW_TTL: Duration = Duration::minutes(5);

/// Expired previews are kept this long so a late confirmation reads 410 rather than 404.
const EXPIRED_RETENTION: Duration = Duration::hours(24);

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/shows/{id}/catch-up/preview", post(preview_catchup))
        .route("/shows/{id}/catch-up", post(commit_catchup))
        .with_state(security)
}

/// `POST /shows/{id}/catch-up/preview`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    pub through_episode_id: Uuid,
}

/// `POST /shows/{id}/catch-up`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatchupRequest {
    pub preview_id: Uuid,
}

/// An episode by position only; previews never carry episode details.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CatchupEpisode {
    pub id: Uuid,
    pub season: i32,
    pub episode: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Through {
    pub season: i32,
    pub episode: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Excluded {
    pub already_watched: Vec<CatchupEpisode>,
    pub future: Vec<CatchupEpisode>,
    pub undated: Vec<CatchupEpisode>,
    pub specials: Vec<CatchupEpisode>,
}

/// C05 catch-up preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub preview_id: Uuid,
    /// RFC 3339 UTC.
    pub expires_at: String,
    pub through: Through,
    pub included: Vec<CatchupEpisode>,
    pub excluded: Excluded,
    pub count: u32,
}

/// The revisions a preview is bound to besides the catalog revision. The tracking revision
/// advances on every watched write or reversal of the show, so it covers all progress rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Revisions {
    tracking: i64,
    library: i64,
}

/// One active catalog episode with the viewer's mark, as a preview classifies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Candidate {
    code: CatchupEpisode,
    release: ReleaseState,
    watched: bool,
}

/// Splits the active episodes ordered through `through` into the catch-up set and its
/// exclusions, each in season, episode order. Specials (season 0) are always excluded, marked
/// regular episodes are already watched, and only released unmarked regular episodes are
/// included: future and undated ones are listed even when the endpoint itself is future.
fn classify(mut candidates: Vec<Candidate>, through: Through) -> (Vec<CatchupEpisode>, Excluded) {
    candidates.sort_by_key(|c| (c.code.season, c.code.episode, c.code.id));
    let (mut included, mut excluded) = (Vec::new(), Excluded::default());
    let in_scope = candidates
        .into_iter()
        .filter(|c| (c.code.season, c.code.episode) <= (through.season, through.episode));
    for candidate in in_scope {
        let list = match candidate {
            Candidate { code, .. } if code.season == 0 => &mut excluded.specials,
            Candidate { watched: true, .. } => &mut excluded.already_watched,
            Candidate { release, .. } => match release {
                ReleaseState::Released => &mut included,
                ReleaseState::Future => &mut excluded.future,
                ReleaseState::Unknown => &mut excluded.undated,
            },
        };
        list.push(candidate.code);
    }
    (included, excluded)
}

async fn preview_catchup(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(show_id): Path<String>,
    ApiJson(request): ApiJson<PreviewRequest>,
) -> Result<Json<Preview>, ApiError> {
    let show_id = path_id(&show_id)?;
    Ok(Json(
        preview(
            &security.pool,
            current.user.id,
            show_id,
            request.through_episode_id,
        )
        .await?,
    ))
}

async fn commit_catchup(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(show_id): Path<String>,
    ApiJson(request): ApiJson<CatchupRequest>,
) -> Result<Json<Value>, ApiError> {
    let show_id = path_id(&show_id)?;
    let user_id = current.user.id;
    let request_hash = idempotency::fingerprint("tracking.catchup", show_id, &request);
    Ok(Json(
        idempotency::transact(&security.pool, user_id, key, request_hash, async |conn| {
            commit_in(conn, user_id, show_id, request.preview_id).await
        })
        .await?,
    ))
}

/// Stores a preview of catching `user_id` up through `through_episode_id`. An unknown show is
/// 404; an endpoint that is not an active regular episode of the show is 422
/// INVALID_EPISODE_SCOPE. Watched progress and the library are never written.
pub async fn preview(
    pool: &PgPool,
    user_id: Uuid,
    show_id: Uuid,
    through_episode_id: Uuid,
) -> Result<Preview, ApiError> {
    sqlx::query(
        "DELETE FROM catchup_previews
         WHERE user_id = $1 AND expires_at <= now() - make_interval(secs => $2)",
    )
    .bind(user_id)
    .bind(EXPIRED_RETENTION.num_seconds() as f64)
    .execute(pool)
    .await?;

    let mut tx = pool.begin().await?;
    // One snapshot for the catalog, marks and revisions the preview is bound to.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    let show: Option<(DateTime<Utc>, i64)> =
        sqlx::query_as("SELECT now(), catalog_revision FROM shows WHERE id = $1")
            .bind(show_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (now, catalog_revision) = show.ok_or_else(ApiError::not_found)?;
    // Archived episodes, directly or through their season, have left active ordering.
    type EpisodeTuple = (
        Uuid,
        i32,
        i32,
        Option<NaiveDate>,
        Option<String>,
        Option<DateTime<Utc>>,
        bool,
    );
    let rows: Vec<EpisodeTuple> = sqlx::query_as(
        "SELECT e.id, se.number, e.number, e.air_date, e.release_timezone,
                e.air_date::timestamp AT TIME ZONE e.release_timezone,
                coalesce(p.watched, false)
         FROM episodes e
         JOIN seasons se ON se.id = e.season_id
         LEFT JOIN episode_progress p ON p.episode_id = e.id AND p.user_id = $1
         WHERE e.show_id = $2 AND e.archived_at IS NULL AND se.archived_at IS NULL",
    )
    .bind(user_id)
    .bind(show_id)
    .fetch_all(&mut *tx)
    .await?;
    let candidates: Vec<Candidate> = rows
        .iter()
        .map(
            |(id, season, number, air_date, zone, starts_at, watched)| Candidate {
                code: CatchupEpisode {
                    id: *id,
                    season: *season,
                    episode: *number,
                },
                release: Schedule::from_catalog(*air_date, zone.as_deref(), *starts_at)
                    .release(now)
                    .state,
                watched: *watched,
            },
        )
        .collect();
    let through = candidates
        .iter()
        .find(|c| c.code.id == through_episode_id && c.code.season > 0)
        .map(|c| Through {
            season: c.code.season,
            episode: c.code.episode,
        })
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidEpisodeScope))?;
    let (included, excluded) = classify(candidates, through);

    let revisions = Revisions {
        tracking: tracking_revision(&mut tx, user_id, show_id).await?,
        library: sqlx::query_scalar(
            "SELECT coalesce((SELECT revision FROM library_entries
                              WHERE user_id = $1 AND show_id = $2), 0)",
        )
        .bind(user_id)
        .bind(show_id)
        .fetch_one(&mut *tx)
        .await?,
    };
    let ids: Vec<Uuid> = included.iter().map(|episode| episode.id).collect();
    let (preview_id, expires_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO catchup_previews
             (user_id, show_id, endpoint_episode_id, episode_ids, revisions, catalog_revision,
              expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7))
         RETURNING id, expires_at",
    )
    .bind(user_id)
    .bind(show_id)
    .bind(through_episode_id)
    .bind(SqlJson(&ids))
    .bind(SqlJson(revisions))
    .bind(catalog_revision)
    .bind(PREVIEW_TTL.num_seconds() as f64)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Preview {
        preview_id,
        expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        through,
        count: included.len() as u32,
        included,
        excluded,
    })
}

/// Confirms the caller's preview: another user's, another show's or an unknown preview is 404,
/// an expired one 410 ACTION_EXPIRED, and one whose catalog, tracking or library revision moved
/// 409 PREVIEW_STALE. Rejections write nothing. A valid empty preview is a no-op.
async fn commit_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    show_id: Uuid,
    preview_id: Uuid,
) -> Result<MutationResult, ApiError> {
    type PreviewTuple = (SqlJson<Vec<Uuid>>, SqlJson<Revisions>, i64, bool);
    let preview: Option<PreviewTuple> = sqlx::query_as(
        "SELECT episode_ids, revisions, catalog_revision, expires_at > now()
         FROM catchup_previews WHERE id = $1 AND user_id = $2 AND show_id = $3",
    )
    .bind(preview_id)
    .bind(user_id)
    .bind(show_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (SqlJson(ids), SqlJson(bound), catalog_revision, open) =
        preview.ok_or_else(ApiError::not_found)?;
    if !open {
        return Err(ApiError::new(ErrorCode::ActionExpired));
    }
    let stale = || ApiError::new(ErrorCode::PreviewStale);

    // The shared first lock, held FOR SHARE rather than KEY SHARE: catalog imports update the
    // show row, so this holds the catalog revision until commit.
    let (now, current_catalog): (DateTime<Utc>, i64) =
        sqlx::query_as("SELECT now(), catalog_revision FROM shows WHERE id = $1 FOR SHARE")
            .bind(show_id)
            .fetch_one(&mut *conn)
            .await?;
    if current_catalog != catalog_revision {
        return Err(stale());
    }
    let tracking = lock_tracking(conn, user_id, show_id).await?;
    if tracking != bound.tracking {
        return Err(stale());
    }
    let entry = LockedEntry::lock(conn, user_id, show_id).await?;
    if entry.revision != bound.library {
        return Err(stale());
    }
    if ids.is_empty() {
        return result(conn, user_id, show_id, now, None, Vec::new(), 0).await;
    }
    let current: Vec<(Uuid, bool, i64)> = sqlx::query_as(
        "SELECT episode_id, watched, revision FROM episode_progress
         WHERE user_id = $1 AND episode_id = ANY($2) ORDER BY episode_id FOR UPDATE",
    )
    .bind(user_id)
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    // The tracking revision already covers every mark; this guards the bound set itself.
    if current.iter().any(|&(_, watched, _)| watched) {
        return Err(stale());
    }
    let revisions: HashMap<Uuid, i64> = current
        .into_iter()
        .map(|(id, _, revision)| (id, revision))
        .collect();

    // Auto-add and upgrade as an individual mark. An absent entry cannot be locked, so a
    // concurrent first save can still insert it: that moved the library revision, so it is stale.
    let mut changes = match entry.store(conn, entry.before.watched(now)).await {
        Err(error) if error.code() == ErrorCode::RevisionConflict => return Err(stale()),
        stored => stored?,
    };
    changes.extend(ids.iter().map(|&id| Change {
        entity: Entity::EpisodeProgress,
        key: id,
        field: "watched",
        before: json!(false),
        after_revision: revisions.get(&id).copied().unwrap_or(0) + 1,
    }));
    let action = actions::record(conn, user_id, show_id, "catch_up", &changes).await?;
    let writes: Vec<(Uuid, bool)> = ids.iter().map(|&id| (id, true)).collect();
    let episodes = write_progress(conn, user_id, &writes, action.map(|a| a.id)).await?;
    bump_tracking(conn, user_id, show_id, tracking).await?;
    let changed = episodes.len() as u32;
    result(conn, user_id, show_id, now, action, episodes, changed).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(n: u128, season: i32, episode: i32, release: ReleaseState) -> Candidate {
        Candidate {
            code: CatchupEpisode {
                id: Uuid::from_u128(n),
                season,
                episode,
            },
            release,
            watched: false,
        }
    }

    fn ids(episodes: &[CatchupEpisode]) -> Vec<u128> {
        episodes.iter().map(|e| e.id.as_u128()).collect()
    }

    #[test]
    fn only_released_unmarked_regular_episodes_through_the_endpoint_are_included() {
        use ReleaseState::*;
        // C09 case 4: E1 marked, E2/E3 released, E4 future, E5 undated endpoint; plus a special,
        // a marked future episode, a later season and a later released episode out of scope.
        let candidates = vec![
            candidate(6, 2, 1, Released),
            candidate(5, 1, 5, Unknown),
            candidate(4, 1, 4, Future),
            candidate(3, 1, 3, Released),
            candidate(2, 1, 2, Released),
            Candidate {
                watched: true,
                ..candidate(1, 1, 1, Released)
            },
            Candidate {
                watched: true,
                ..candidate(7, 0, 1, Released)
            },
            candidate(8, 0, 2, Future),
        ];
        let through = Through {
            season: 1,
            episode: 5,
        };
        let (included, excluded) = classify(candidates.clone(), through);
        assert_eq!(ids(&included), [2, 3]);
        assert_eq!(ids(&excluded.already_watched), [1]);
        assert_eq!(ids(&excluded.future), [4]);
        assert_eq!(ids(&excluded.undated), [5]);
        assert_eq!(ids(&excluded.specials), [7, 8], "marked or not");

        // A marked future episode is already watched; a future endpoint excludes itself.
        let mut marked = candidates;
        marked[2].watched = true;
        let (included, excluded) = classify(
            marked,
            Through {
                season: 1,
                episode: 4,
            },
        );
        assert_eq!(ids(&included), [2, 3]);
        assert_eq!(ids(&excluded.already_watched), [1, 4]);
        assert!(excluded.future.is_empty() && excluded.undated.is_empty());
    }

    #[test]
    fn a_later_season_endpoint_spans_earlier_seasons_in_order() {
        use ReleaseState::*;
        let candidates = vec![
            candidate(3, 2, 2, Released),
            candidate(2, 2, 1, Released),
            candidate(1, 1, 10, Released),
        ];
        let (included, excluded) = classify(
            candidates,
            Through {
                season: 2,
                episode: 1,
            },
        );
        assert_eq!(ids(&included), [1, 2]);
        assert_eq!(excluded, Excluded::default());
    }
}
