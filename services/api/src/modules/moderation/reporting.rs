//! R31 comment reports (C07): `POST /comments/{id}/reports`. A report needs a comment the viewer
//! can read now: the thread passes the privacy gate (404 otherwise) and the spoiler gate (403
//! SPOILER_LOCKED), and the comment is live and visible to them. Each viewer has at most one open
//! report per comment; repeating returns it. The report hides the comment from the reporter at
//! once and leaves it for everyone else until an operator resolves it.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson},
    modules::{
        auth::session::VerifiedUser,
        discussions::{lock_members, unlocked, visible_comment},
        policy::grants::SessionKey,
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/comments/{id}/reports", post(report))
        .with_state(security)
}

/// The 0005 `reports.reason` values.
const REASONS: [&str; 4] = ["later_episode", "abuse", "spam", "other"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportRequest {
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportResult {
    report_id: Uuid,
}

async fn report(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<ReportRequest>,
) -> Result<(StatusCode, Json<ReportResult>), ApiError> {
    let comment_id = Uuid::try_parse(&id).map_err(|_| ApiError::not_found())?;
    let reason = REASONS
        .into_iter()
        .find(|reason| *reason == request.reason)
        .ok_or_else(|| {
            ApiError::new(ErrorCode::ValidationError)
                .with_field("reason", "Choose later_episode, abuse, spam or other.")
        })?;
    let key = SessionKey::of(&current);
    let viewer = key.user_id();
    let mut tx = security.pool.begin().await?;
    let discussion_id: Uuid =
        sqlx::query_scalar("SELECT discussion_id FROM comments WHERE id = $1")
            .bind(comment_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    lock_members(&mut tx, viewer, discussion_id).await?;
    unlocked(&mut tx, &key, discussion_id).await?;

    // The open report already hides the comment from the reporter, so look for it first.
    if let Some(report_id) = open_report(&mut tx, viewer, comment_id).await? {
        tx.commit().await?;
        return Ok(created(report_id));
    }
    let visible: bool = sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM comments c
                        WHERE c.id = $2 AND c.removed_at IS NULL AND ",
        visible_comment!(),
        ")"
    ))
    .bind(viewer)
    .bind(comment_id)
    .fetch_one(&mut *tx)
    .await?;
    if !visible {
        return Err(ApiError::not_found());
    }
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO reports (reporter_id, comment_id, reason) VALUES ($1, $2, $3)
         ON CONFLICT (reporter_id, comment_id) WHERE state = 'open' DO NOTHING
         RETURNING id",
    )
    .bind(viewer)
    .bind(comment_id)
    .bind(reason)
    .fetch_optional(&mut *tx)
    .await?;
    // A concurrent report by the same viewer won the insert; this new statement sees it.
    let report_id = match inserted {
        Some(id) => id,
        None => open_report(&mut tx, viewer, comment_id)
            .await?
            .ok_or_else(|| ApiError::unavailable("report conflict"))?,
    };
    tx.commit().await?;
    Ok(created(report_id))
}

async fn open_report(
    conn: &mut PgConnection,
    reporter: Uuid,
    comment_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM reports WHERE reporter_id = $1 AND comment_id = $2 AND state = 'open'",
    )
    .bind(reporter)
    .bind(comment_id)
    .fetch_optional(conn)
    .await
}

fn created(report_id: Uuid) -> (StatusCode, Json<ReportResult>) {
    (StatusCode::CREATED, Json(ReportResult { report_id }))
}
