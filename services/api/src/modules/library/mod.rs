//! R15 personal library (C05), relative to `/api/v1`: `GET /library`, `PUT /library/{showId}`
//! and `PATCH /library/{showId}`. Integration (R24/R38) mounts [`routes`] behind the C03 security
//! layers. [`actions`] and [`idempotency`] are the shared write bookkeeping that tracking
//! mutations (R16) extend; [`repository::items`] and [`repository::saved_watching`] provide
//! LibraryItem projections and the Home source (R18).

pub mod actions;
pub mod dto;
pub mod idempotency;
pub mod repository;
pub mod service;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use dto::{LibraryPage, SaveRequest, Status, StatusRequest, expected_revision};
use idempotency::IdempotencyKey;
use repository::{PageQuery, Position};
use serde::Deserialize;
use serde_json::Value;
use service::{Mutation, Write};
use uuid::Uuid;

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson},
    modules::auth::session::VerifiedUser,
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/library", get(list))
        .route(
            "/library/{showId}",
            axum::routing::put(save).patch(set_status),
        )
        .with_state(security)
}

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 50;
const MAX_QUERY_CHARS: usize = 100;
const MAX_CURSOR_LEN: usize = 2048;

/// Raw query parameters, so invalid values are field errors rather than request-format errors.
#[derive(Deserialize)]
struct ListParams {
    status: Option<String>,
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

async fn list(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<ListParams>, QueryRejection>,
) -> Result<Json<LibraryPage>, ApiError> {
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let status = params
        .status
        .as_deref()
        .map(Status::from_request)
        .transpose()?;
    let q = params.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    if q.is_some_and(|q| q.chars().count() > MAX_QUERY_CHARS) {
        return Err(
            ApiError::new(ErrorCode::ValidationError).with_field("q", "Use up to 100 characters.")
        );
    }
    let limit = match params.limit.as_deref() {
        None => DEFAULT_LIMIT,
        Some(text) => text
            .parse()
            .ok()
            .filter(|limit| (1..=MAX_LIMIT).contains(limit))
            .ok_or_else(|| {
                ApiError::new(ErrorCode::ValidationError)
                    .with_field("limit", "Use a limit from 1 to 50.")
            })?,
    };
    let after = params.cursor.as_deref().map(decode_cursor).transpose()?;

    let user_id = current.user.id;
    let mut conn = security.pool.acquire().await?;
    let counts = repository::counts(&mut conn, user_id).await?;
    let mut positions = repository::page(
        &mut conn,
        user_id,
        &PageQuery {
            status,
            q,
            after,
            limit,
        },
    )
    .await?;
    let next_cursor = if positions.len() as i64 > limit {
        positions.truncate(limit as usize);
        positions.last().map(encode_cursor)
    } else {
        None
    };
    let ids: Vec<Uuid> = positions.iter().map(|p| p.show_id).collect();
    let items = repository::items(&mut conn, user_id, &ids, Utc::now()).await?;
    Ok(Json(LibraryPage {
        items,
        next_cursor,
        counts,
    }))
}

async fn save(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(show_id): Path<String>,
    ApiJson(request): ApiJson<SaveRequest>,
) -> Result<Json<Value>, ApiError> {
    let show_id = path_show_id(&show_id)?;
    // Removal ignores a valid status: it keeps the manual one (C05).
    let status = request
        .status
        .as_deref()
        .map(Status::from_request)
        .transpose()?;
    let mutation = Mutation {
        user_id: current.user.id,
        show_id,
        key,
        request_hash: idempotency::fingerprint("library.save", show_id, &request),
        expected_revision: expected_revision(request.expected_revision)?,
        write: Write::Save {
            saved: request.saved,
            status,
        },
    };
    Ok(Json(service::apply(&security.pool, mutation).await?))
}

async fn set_status(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(show_id): Path<String>,
    ApiJson(request): ApiJson<StatusRequest>,
) -> Result<Json<Value>, ApiError> {
    let show_id = path_show_id(&show_id)?;
    let mutation = Mutation {
        user_id: current.user.id,
        show_id,
        key,
        request_hash: idempotency::fingerprint("library.status", show_id, &request),
        expected_revision: expected_revision(request.expected_revision)?,
        write: Write::SetStatus(Status::from_request(&request.status)?),
    };
    Ok(Json(service::apply(&security.pool, mutation).await?))
}

/// A show ID that is not a UUID cannot name a show: 404, like an unknown one.
fn path_show_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(value).map_err(|_| ApiError::not_found())
}

fn encode_cursor(position: &Position) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}.{}",
        position.saved_at.timestamp_micros(),
        position.show_id.simple()
    ))
}

/// 400 for anything this server did not issue.
fn decode_cursor(cursor: &str) -> Result<Position, ApiError> {
    let decode = || {
        if cursor.len() > MAX_CURSOR_LEN {
            return None;
        }
        let text = String::from_utf8(URL_SAFE_NO_PAD.decode(cursor).ok()?).ok()?;
        let (micros, id) = text.split_once('.')?;
        Some(Position {
            saved_at: DateTime::from_timestamp_micros(micros.parse().ok()?)?,
            show_id: Uuid::try_parse(id).ok()?,
        })
    };
    decode().ok_or_else(ApiError::malformed)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_tampering() {
        let position = Position {
            saved_at: Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap()
                + chrono::Duration::microseconds(123_456),
            show_id: Uuid::from_u128(42),
        };
        let cursor = encode_cursor(&position);
        assert_eq!(decode_cursor(&cursor).unwrap(), position);
        for invalid in [
            "",
            "not base64!",
            &URL_SAFE_NO_PAD.encode("12.not-a-uuid"),
            &URL_SAFE_NO_PAD.encode("x.00000000000000000000000000000001"),
            &"A".repeat(MAX_CURSOR_LEN + 4),
        ] {
            assert_eq!(
                decode_cursor(invalid).unwrap_err().status().as_u16(),
                400,
                "{invalid}"
            );
        }
    }
}
