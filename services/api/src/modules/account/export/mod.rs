//! R35 private JSON data export (C08), relative to `/api/v1`: `POST /me/export`,
//! `GET /me/export/{jobId}` and `GET /me/export/{jobId}/download`. A request queues one `export`
//! job per user (a repeat returns the active one); the [`Exporter`] worker builds the
//! [`document`], keeps it in the private [`ExportStore`] for 24 hours and deletes it afterwards.
//!
//! Only the owner can read a job: another user's job ID is 404 like an unknown one. The artifact
//! is streamed through this authenticated route only; no URL or path to it is ever returned.
//! Integration (R38) mounts [`routes`] behind the C03 layers while the beta flag is on and runs
//! the [`Exporter`] alongside the other workers, both over one [`ExportStore`] directory.

pub mod document;
pub mod store;
mod worker;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::PgPool;
pub use store::ExportStore;
use uuid::Uuid;
pub use worker::{Exporter, Failure, LEASE, MAX_ATTEMPTS, Pass, TTL};

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::auth::session::VerifiedUser,
};

#[derive(Clone)]
struct Exports {
    pool: PgPool,
    store: ExportStore,
}

pub fn routes(security: Security, store: ExportStore) -> Router {
    Router::new()
        .route("/me/export", post(request))
        .route("/me/export/{jobId}", get(status))
        .route("/me/export/{jobId}/download", get(download))
        .with_state(Exports {
            pool: security.pool,
            store,
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportJob {
    job_id: Uuid,
    /// The ExportJob schema reports a request as queued even when the returned active job has
    /// already been claimed; `GET /me/export/{jobId}` has the current state.
    state: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportStatus {
    state: String,
    expires_at: Option<String>,
}

const ATTACHMENT: &str = "attachment; filename=\"scenecask-export.json\"";

/// `POST /me/export`: queues an export, or returns the user's active one.
async fn request(
    VerifiedUser(current): VerifiedUser,
    State(exports): State<Exports>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<(StatusCode, Json<ExportJob>), ApiError> {
    let user_id = current.user.id;
    // The active job can finish between a conflicting insert and the read; try again then.
    for _ in 0..3 {
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO jobs (user_id, kind) VALUES ($1, 'export')
             ON CONFLICT (user_id, kind)
                 WHERE kind IN ('export', 'delete') AND state IN ('queued', 'running')
                 DO NOTHING
             RETURNING id",
        )
        .bind(user_id)
        .fetch_optional(&exports.pool)
        .await?;
        let job_id = match inserted {
            Some(id) => Some(id),
            None => {
                sqlx::query_scalar(
                    "SELECT id FROM jobs
                     WHERE user_id = $1 AND kind = 'export' AND state IN ('queued', 'running')",
                )
                .bind(user_id)
                .fetch_optional(&exports.pool)
                .await?
            }
        };
        if let Some(job_id) = job_id {
            return Ok((
                StatusCode::ACCEPTED,
                Json(ExportJob {
                    job_id,
                    state: "queued",
                }),
            ));
        }
    }
    Err(ApiError::new(ErrorCode::ServiceUnavailable))
}

/// The owner's export job: state, whether its artifact is past expiry, and the stored file.
struct OwnedJob {
    state: String,
    expires_at: Option<DateTime<Utc>>,
    expired: bool,
    result_path: Option<String>,
}

/// `job_id` if it is an export owned by `user_id`; 404 for anything else, including a path ID
/// that is not a UUID, so another user's job is indistinguishable from an unknown one.
async fn owned(pool: &PgPool, user_id: Uuid, job_id: &str) -> Result<OwnedJob, ApiError> {
    let job_id = Uuid::try_parse(job_id).map_err(|_| ApiError::not_found())?;
    type Row = (String, Option<DateTime<Utc>>, bool, Option<String>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT state, expires_at, coalesce(expires_at <= now(), false), result_path
         FROM jobs WHERE id = $1 AND user_id = $2 AND kind = 'export'",
    )
    .bind(job_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let (state, expires_at, expired, result_path) = row.ok_or_else(ApiError::not_found)?;
    Ok(OwnedJob {
        state,
        expires_at,
        expired,
        result_path,
    })
}

/// `GET /me/export/{jobId}`.
async fn status(
    VerifiedUser(current): VerifiedUser,
    State(exports): State<Exports>,
    Path(job_id): Path<String>,
) -> Result<Json<ExportStatus>, ApiError> {
    let job = owned(&exports.pool, current.user.id, &job_id).await?;
    Ok(Json(ExportStatus {
        state: job.state,
        expires_at: job
            .expires_at
            .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true)),
    }))
}

/// `GET /me/export/{jobId}/download`: the artifact as a JSON attachment. 409 until ready, 410
/// once expired.
async fn download(
    VerifiedUser(current): VerifiedUser,
    State(exports): State<Exports>,
    Path(job_id): Path<String>,
) -> Result<Response, ApiError> {
    let job = owned(&exports.pool, current.user.id, &job_id).await?;
    if job.state != "ready" {
        return Err(ApiError::new(ErrorCode::RevisionConflict));
    }
    let Some(name) = job.result_path.filter(|_| !job.expired) else {
        return Err(ApiError::new(ErrorCode::ActionExpired));
    };
    let bytes = match exports.store.read(&name).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(category = "artifact_missing", "export download failed");
            return Err(ApiError::new(ErrorCode::ActionExpired));
        }
        Err(_) => return Err(ApiError::unavailable("storage")),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CONTENT_DISPOSITION, ATTACHMENT),
        ],
        bytes,
    )
        .into_response())
}
