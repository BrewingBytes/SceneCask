//! R18 spoiler reveals (C07), relative to `/api/v1`: `POST /reveals` and
//! `DELETE /reveals/{scope}/{resourceId}`. Integration (R24/R38) mounts [`routes`] behind the C03
//! security layers. [`grants`] loads a session's grants for projections and [`relationships`]
//! reads the follow/block facts the privacy policy needs, so a discussion reveal is checked against
//! current membership before any grant exists. A grant only relaxes the spoiler gate for its own
//! session and resource; it never marks progress or grants membership.

pub mod grants;
pub mod relationships;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
};
use chrono::SecondsFormat;
use grants::SessionKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::spoilers::RevealScope,
    error::{ApiError, ErrorCode},
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::auth::session::VerifiedUser,
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/reveals", post(reveal))
        .route("/reveals/{scope}/{resourceId}", delete(hide))
        .with_state(security)
}

/// `POST /reveals`. The scope stays a string so an unknown one gets a field error.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevealRequest {
    scope: String,
    resource_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reveal {
    scope: &'static str,
    resource_id: Uuid,
    /// RFC 3339 UTC.
    expires_at: String,
}

fn parse_scope(value: &str) -> Option<RevealScope> {
    [RevealScope::EpisodeDetails, RevealScope::Discussion]
        .into_iter()
        .find(|scope| scope.as_str() == value)
}

async fn reveal(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    ApiJson(request): ApiJson<RevealRequest>,
) -> Result<(StatusCode, Json<Reveal>), ApiError> {
    let scope = parse_scope(&request.scope).ok_or_else(|| {
        ApiError::new(ErrorCode::ValidationError)
            .with_field("scope", "Choose episode_details or discussion.")
    })?;
    let key = SessionKey::of(&current);
    let mut conn = security.pool.acquire().await?;
    // Privacy before grant: an unknown or inaccessible resource is concealed as 404.
    let accessible = match scope {
        RevealScope::EpisodeDetails => {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM episodes WHERE id = $1)")
                .bind(request.resource_id)
                .fetch_one(&mut *conn)
                .await?
        }
        RevealScope::Discussion => {
            relationships::discussion_member(&mut conn, key.user_id(), request.resource_id)
                .await?
                .is_some()
        }
    };
    if !accessible {
        return Err(ApiError::not_found());
    }
    let expires_at = grants::create(&mut conn, &key, scope, request.resource_id)
        .await?
        .ok_or_else(ApiError::auth_required)?;
    Ok((
        StatusCode::CREATED,
        Json(Reveal {
            scope: scope.as_str(),
            resource_id: request.resource_id,
            expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        }),
    ))
}

/// Hide again. Idempotent: hiding something not revealed is still 204.
async fn hide(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path((scope, resource_id)): Path<(String, String)>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<StatusCode, ApiError> {
    // A path that cannot name a grant is an unknown resource.
    let scope = parse_scope(&scope).ok_or_else(ApiError::not_found)?;
    let resource_id = Uuid::try_parse(&resource_id).map_err(|_| ApiError::not_found())?;
    let mut conn = security.pool.acquire().await?;
    grants::delete(&mut conn, &SessionKey::of(&current), scope, resource_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
