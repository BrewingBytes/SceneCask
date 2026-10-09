//! R16 tracking writes (C05), relative to `/api/v1`: `PUT /progress/episodes/{episodeId}`,
//! `DELETE /shows/{id}/history` and `POST /actions/{id}/undo`. Integration (R24/R38) mounts
//! [`routes`] behind the C03 security layers. Every write requires an `Idempotency-Key` and shares
//! the library action recorder, so Undo reverses library and episode changes alike.

pub mod home;
pub mod mutations;
pub mod undo;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, post, put},
};
use mutations::{HistoryRequest, Keyed, ProgressRequest};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::ApiError,
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::{
        auth::session::VerifiedUser,
        library::{
            dto::expected_revision,
            idempotency::{self, IdempotencyKey},
        },
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/progress/episodes/{episodeId}", put(set_progress))
        .route("/shows/{id}/history", delete(erase_history))
        .route("/actions/{id}/undo", post(undo_action))
        .with_state(security)
}

async fn set_progress(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(episode_id): Path<String>,
    ApiJson(request): ApiJson<ProgressRequest>,
) -> Result<Json<Value>, ApiError> {
    let episode_id = path_id(&episode_id)?;
    let keyed = Keyed {
        user_id: current.user.id,
        key,
        request_hash: idempotency::fingerprint("progress.episode", episode_id, &request),
        expected_revision: expected_revision(request.expected_revision)?,
    };
    let result = mutations::set_episode(&security.pool, keyed, episode_id, request.watched);
    Ok(Json(result.await?))
}

async fn erase_history(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(show_id): Path<String>,
    ApiJson(request): ApiJson<HistoryRequest>,
) -> Result<Json<Value>, ApiError> {
    let show_id = path_id(&show_id)?;
    let keyed = Keyed {
        user_id: current.user.id,
        key,
        request_hash: idempotency::fingerprint("tracking.history", show_id, &request),
        expected_revision: expected_revision(request.expected_revision)?,
    };
    Ok(Json(
        mutations::erase_history(&security.pool, keyed, show_id).await?,
    ))
}

async fn undo_action(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    key: IdempotencyKey,
    Path(action_id): Path<String>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<Json<Value>, ApiError> {
    let action_id = path_id(&action_id)?;
    let request_hash = idempotency::fingerprint("actions.undo", action_id, &());
    Ok(Json(
        undo::undo(
            &security.pool,
            current.user.id,
            key,
            request_hash,
            action_id,
        )
        .await?,
    ))
}

/// A path ID that is not a UUID cannot name a resource: 404, like an unknown one.
fn path_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(value).map_err(|_| ApiError::not_found())
}
