//! R26 privacy switch (C06): `PATCH /me/privacy`. Visibility is the only stored fact; every
//! library, activity and profile read applies [`crate::domain::access`] to it at request time, so
//! a switch to private hides the library from nonfollowers on their next read. Neither direction
//! touches edges: approved follows stay, and pending requests stay pending after a switch to
//! public until approved or withdrawn (no silent batch conversion).

use axum::{Json, Router, extract::State, routing::patch};
use serde::{Deserialize, Serialize};

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson},
    modules::auth::session::VerifiedUser,
};

/// Visibility stays a string so an unknown one gets a field error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivacyRequest {
    visibility: String,
}

#[derive(Debug, Serialize)]
struct PrivacyResult {
    visibility: &'static str,
}

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/me/privacy", patch(update_privacy))
        .with_state(security)
}

async fn update_privacy(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    ApiJson(request): ApiJson<PrivacyRequest>,
) -> Result<Json<PrivacyResult>, ApiError> {
    let visibility = match request.visibility.as_str() {
        "private" => "private",
        "public" => "public",
        _ => {
            return Err(ApiError::new(ErrorCode::ValidationError)
                .with_field("visibility", "Choose private or public."));
        }
    };
    // The row lock this update takes is the one follows and blocks take with `lock_pair`, so a
    // concurrent follow sees either the old visibility or the new one, never a mix.
    sqlx::query("UPDATE users SET visibility = $2 WHERE id = $1")
        .bind(current.user.id)
        .bind(visibility)
        .execute(&security.pool)
        .await?;
    Ok(Json(PrivacyResult { visibility }))
}
