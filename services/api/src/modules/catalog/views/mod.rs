//! R18 viewer-filtered catalog reads (C05, C07 episode gate), relative to `/api/v1`:
//! `GET /shows/{id}`, `GET /shows/{id}/episodes` and `GET /episodes/{id}`. Integration (R24/R38)
//! mounts [`routes`] behind the C03 security layers, which mark every response private, no-store.
//! Episode details appear only when the viewer individually watched the episode or this session
//! holds an `episode_details` reveal; otherwise the `details` key is absent.

pub mod repository;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::get,
};
use chrono::Utc;
use repository::{EpisodePosition, EpisodeQuery, ShowView};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::spoilers::EpisodeView,
    error::{ApiError, ErrorCode},
    middleware::Security,
    modules::{
        auth::session::VerifiedUser,
        library::{decode_opaque, encode_opaque, page_limit},
        policy::grants::SessionKey,
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/shows/{id}", get(show))
        .route("/shows/{id}/episodes", get(list_episodes))
        .route("/episodes/{id}", get(episode))
        .with_state(security)
}

/// Episode listing default page size (C05), also the C03 maximum.
const DEFAULT_LIMIT: i64 = 50;

/// Raw query parameters, so invalid values are field errors rather than request-format errors.
#[derive(Deserialize)]
struct ListParams {
    season: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EpisodesPage {
    items: Vec<EpisodeView>,
    next_cursor: Option<String>,
}

async fn show(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
) -> Result<Json<ShowView>, ApiError> {
    let show_id = path_id(&id)?;
    let mut conn = security.pool.acquire().await?;
    repository::show(&mut conn, current.user.id, show_id, Utc::now())
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn list_episodes(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    params: Result<Query<ListParams>, QueryRejection>,
) -> Result<Json<EpisodesPage>, ApiError> {
    let show_id = path_id(&id)?;
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let season = params
        .season
        .as_deref()
        .map(|text| {
            text.parse()
                .ok()
                .filter(|season: &i32| *season >= 0)
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::ValidationError)
                        .with_field("season", "Use a season number of 0 or more.")
                })
        })
        .transpose()?;
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = params.cursor.as_deref().map(decode_cursor).transpose()?;

    let key = SessionKey::of(&current);
    let mut conn = security.pool.acquire().await?;
    if !repository::show_exists(&mut conn, show_id).await? {
        return Err(ApiError::not_found());
    }
    let query = EpisodeQuery {
        season,
        after,
        limit,
    };
    let (items, next) = repository::episodes(&mut conn, &key, show_id, &query, Utc::now()).await?;
    Ok(Json(EpisodesPage {
        items,
        next_cursor: next.as_ref().map(encode_cursor),
    }))
}

async fn episode(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
) -> Result<Json<EpisodeView>, ApiError> {
    let episode_id = path_id(&id)?;
    let key = SessionKey::of(&current);
    let mut conn = security.pool.acquire().await?;
    repository::episode(&mut conn, &key, episode_id, Utc::now())
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

/// A path ID that is not a UUID cannot name a resource: 404, like an unknown one.
fn path_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(value).map_err(|_| ApiError::not_found())
}

fn encode_cursor(position: &EpisodePosition) -> String {
    encode_opaque(&format!(
        "{}.{}.{}",
        position.season,
        position.number,
        position.id.simple()
    ))
}

fn decode_cursor(cursor: &str) -> Result<EpisodePosition, ApiError> {
    decode_opaque(cursor, |text| {
        let mut parts = text.splitn(3, '.');
        let season = parts.next()?.parse().ok().filter(|season| *season >= 0)?;
        let number = parts.next()?.parse().ok().filter(|number| *number >= 0)?;
        Some(EpisodePosition {
            season,
            number,
            id: Uuid::try_parse(parts.next()?).ok()?,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_tampering() {
        let position = EpisodePosition {
            season: 2,
            number: 7,
            id: Uuid::from_u128(42),
        };
        assert_eq!(decode_cursor(&encode_cursor(&position)).unwrap(), position);
        for invalid in [
            "",
            "not base64!",
            &encode_opaque("1.2"),
            &encode_opaque("-1.2.00000000000000000000000000000001"),
            &encode_opaque("1.x.00000000000000000000000000000001"),
            &encode_opaque("1.2.not-a-uuid"),
            &"A".repeat(4096),
        ] {
            assert_eq!(
                decode_cursor(invalid).unwrap_err().status().as_u16(),
                400,
                "{invalid}"
            );
        }
    }
}
