//! `GET /session` and `POST /auth/logout` (C04), relative to `/api/v1`.

use super::{
    RequestSession, SessionUser, cookie,
    cookie::Kind,
    secret::{CsrfPurpose, Secret},
    store,
};
use crate::{
    error::ApiError,
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
};
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub user: Option<UserDto>,
    pub csrf_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub handle: Option<String>,
    pub visibility: String,
    pub verified: bool,
    pub role: super::Role,
}

impl From<&SessionUser> for UserDto {
    fn from(user: &SessionUser) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            handle: user.handle.clone(),
            visibility: user.visibility.clone(),
            verified: user.verified,
            role: user.role,
        }
    }
}

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/session", get(get_session))
        .route("/auth/logout", post(logout))
        .with_state(security)
}

/// Returns the signed-in user and session-paired CSRF token, or `user:null` with a token bound to
/// an anonymous CSRF cookie (issued here when absent). The anonymous cookie is not a session.
async fn get_session(
    State(security): State<Security>,
    session: RequestSession,
) -> Result<Response, ApiError> {
    if let Some(current) = session.current() {
        return Ok(Json(SessionDto {
            user: Some(UserDto::from(&current.user)),
            csrf_token: current.csrf_token(),
        })
        .into_response());
    }
    let (secret, issued) = match session.anonymous {
        Some(secret) => (secret, false),
        None => (Secret::generate()?, true),
    };
    let mut response = Json(SessionDto {
        user: None,
        csrf_token: secret.csrf_token(CsrfPurpose::Anonymous),
    })
    .into_response();
    if issued {
        response.headers_mut().append(
            header::SET_COOKIE,
            cookie::set(
                Kind::AnonymousCsrf,
                security.config.cookie_mode,
                &secret,
                None,
            ),
        );
    }
    Ok(response)
}

/// Revokes the current session and its reveal grants and expires the cookie. Without a live
/// session this is a no-op that still returns 204.
async fn logout(
    State(security): State<Security>,
    session: RequestSession,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<Response, ApiError> {
    if let Some(current) = session.current() {
        store::revoke(&security.pool, &current.id_hash()).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            cookie::clear(Kind::Session, security.config.cookie_mode),
        )],
    )
        .into_response())
}
