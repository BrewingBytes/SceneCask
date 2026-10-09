//! R09 sign-in methods (C04), relative to `/api/v1`: `GET /me/identities` and
//! `DELETE /me/identities/google`. Unlinking needs a session reauthenticated within
//! [`REAUTH_WINDOW`] and never removes an account's last sign-in method; it does not delete the
//! account or end any session.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::Serialize;
use sqlx::Row;

use super::{
    google::{Google, lock_account},
    password::REAUTH_WINDOW,
    session::AuthUser,
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::Security,
};

#[derive(Clone)]
struct IdentitiesState {
    security: Security,
    /// The issuer Google identities are stored under.
    issuer: Arc<str>,
}

pub fn routes(security: Security, google: &Google) -> Router {
    Router::new()
        .route("/me/identities", get(identities))
        .route("/me/identities/google", delete(unlink_google))
        .with_state(IdentitiesState {
            security,
            issuer: google.issuer().into(),
        })
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct IdentitiesDto {
    password_enabled: bool,
    google_linked: bool,
}

async fn identities(
    State(state): State<IdentitiesState>,
    AuthUser(current): AuthUser,
) -> Result<Response, ApiError> {
    let row = sqlx::query(
        "SELECT EXISTS (SELECT 1 FROM password_credentials WHERE user_id = $1) AS password,
                EXISTS (SELECT 1 FROM external_identities WHERE user_id = $1 AND issuer = $2)
                    AS google",
    )
    .bind(current.user.id)
    .bind(&*state.issuer)
    .fetch_one(&state.security.pool)
    .await?;
    Ok(Json(IdentitiesDto {
        password_enabled: row.try_get("password")?,
        google_linked: row.try_get("google")?,
    })
    .into_response())
}

/// Removes the Google identity if another sign-in method remains: 409 LAST_SIGNIN_METHOD
/// otherwise. Already unlinked is 204. A stale session gets 401 AUTH_REQUIRED, the cue to
/// reauthenticate first.
async fn unlink_google(
    State(state): State<IdentitiesState>,
    AuthUser(current): AuthUser,
) -> Result<Response, ApiError> {
    if !current.reauthenticated_within(REAUTH_WINDOW) {
        return Err(ApiError::auth_required());
    }
    let mut tx = state.security.pool.begin().await?;
    lock_account(&mut tx, current.user.id).await?;
    let row = sqlx::query(
        "SELECT EXISTS (SELECT 1 FROM external_identities WHERE user_id = $1 AND issuer = $2)
                    AS linked,
                EXISTS (SELECT 1 FROM password_credentials WHERE user_id = $1)
                OR EXISTS (SELECT 1 FROM external_identities WHERE user_id = $1 AND issuer <> $2)
                    AS other_method",
    )
    .bind(current.user.id)
    .bind(&*state.issuer)
    .fetch_one(&mut *tx)
    .await?;
    let linked: bool = row.try_get("linked")?;
    let other_method: bool = row.try_get("other_method")?;
    if linked && !other_method {
        return Err(ApiError::new(ErrorCode::LastSigninMethod));
    }
    sqlx::query("DELETE FROM external_identities WHERE user_id = $1 AND issuer = $2")
        .bind(current.user.id)
        .bind(&*state.issuer)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
