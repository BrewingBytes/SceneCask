//! C04 server-managed cookie sessions: opaque secrets with server-side hashes, idle/absolute
//! expiry, rotation on authentication, revocation, identity extraction and guards.

pub mod cookie;
mod routes;
pub mod secret;
pub mod store;

use std::time::Duration;

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderValue, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use cookie::Kind;
pub use routes::{SessionDto, UserDto, routes};
use secret::{CsrfPurpose, Secret};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, SecurityConfig},
};

#[derive(Clone, Copy, Debug)]
pub struct Lifetimes {
    pub idle: Duration,
    pub absolute: Duration,
    /// Minimum gap between `last_seen_at` writes, so reads do not write on every request.
    pub touch_interval: Duration,
}

impl Default for Lifetimes {
    fn default() -> Self {
        const DAY: u64 = 24 * 60 * 60;
        Self {
            idle: Duration::from_secs(7 * DAY),
            absolute: Duration::from_secs(30 * DAY),
            touch_interval: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Member,
    Operator,
}

#[derive(Clone, Debug)]
pub struct SessionUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub handle: Option<String>,
    pub visibility: String,
    pub verified: bool,
    pub role: Role,
}

/// A live authenticated session. The secret is the cookie value: never log it.
#[derive(Clone, Debug)]
pub struct CurrentSession {
    secret: Secret,
    pub user: SessionUser,
    reauthenticated_secs_ago: Option<f64>,
}

impl CurrentSession {
    /// The `sessions.id_hash` (also `reveal_grants.session_id`) of this session.
    pub fn id_hash(&self) -> Vec<u8> {
        self.secret.hash()
    }

    /// The X-CSRF-Token paired to this session.
    pub fn csrf_token(&self) -> String {
        self.secret.csrf_token(CsrfPurpose::Session)
    }

    /// Whether the session authenticated or reauthenticated within `window` (C04: 5 minutes for
    /// link/unlink/delete).
    pub fn reauthenticated_within(&self, window: Duration) -> bool {
        self.reauthenticated_secs_ago
            .is_some_and(|seconds| seconds <= window.as_secs_f64())
    }
}

/// The identity resolved for a request by [`load`].
#[derive(Clone, Debug, Default)]
pub struct RequestSession {
    current: Option<CurrentSession>,
    anonymous: Option<Secret>,
}

impl RequestSession {
    pub fn current(&self) -> Option<&CurrentSession> {
        self.current.as_ref()
    }
}

/// Resolves the session cookie into a [`RequestSession`] extension. An expired, revoked or
/// disabled-user session is deleted (revoking its grants), treated as anonymous, and its cookie
/// cleared on the response. Database failure fails closed with 503.
pub async fn load(State(security): State<Security>, mut request: Request, next: Next) -> Response {
    let mode = security.config.cookie_mode;
    let presented = cookie::read(request.headers(), Kind::Session, mode);
    let mut stale = false;
    let current = match presented {
        None => None,
        Some(secret) => {
            let hash = secret.hash();
            match store::lookup(&security.pool, &hash, &security.config.lifetimes).await {
                Ok(Some(found)) => Some(CurrentSession {
                    secret,
                    user: found.user,
                    reauthenticated_secs_ago: found.reauthenticated_secs_ago,
                }),
                Ok(None) => {
                    if let Err(error) = store::revoke(&security.pool, &hash).await {
                        return ApiError::from(error).into_response();
                    }
                    stale = true;
                    None
                }
                Err(error) => return ApiError::from(error).into_response(),
            }
        }
    };
    let anonymous = cookie::read(request.headers(), Kind::AnonymousCsrf, mode);
    request
        .extensions_mut()
        .insert(RequestSession { current, anonymous });
    let mut response = next.run(request).await;
    if stale && !cookie::is_set(response.headers(), Kind::Session, mode) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie::clear(Kind::Session, mode));
    }
    response
}

/// A newly established session: send `cookie` as Set-Cookie and `session` as the body.
pub struct StartedSession {
    pub cookie: HeaderValue,
    pub session: SessionDto,
}

/// Establishes a session for `user_id` after successful authentication (login, verification,
/// Google sign-in), rotating away `previous`. Pass a transaction to make the rotation atomic
/// with the caller's writes. Disabled users get 401; unverified users get 403 EMAIL_UNVERIFIED.
pub async fn start_session(
    conn: &mut PgConnection,
    config: &SecurityConfig,
    previous: Option<&CurrentSession>,
    user_id: Uuid,
) -> Result<StartedSession, ApiError> {
    let (secret, user) = store::create(
        conn,
        user_id,
        previous.map(|current| &current.secret),
        &config.lifetimes,
    )
    .await?;
    Ok(StartedSession {
        cookie: cookie::set(
            Kind::Session,
            config.cookie_mode,
            &secret,
            Some(config.lifetimes.absolute),
        ),
        session: SessionDto {
            user: Some(UserDto::from(&user)),
            csrf_token: secret.csrf_token(CsrfPurpose::Session),
        },
    })
}

/// Rotates the current session after a successful reauthentication (password or Google reauth
/// intent) and marks it fresh. The old cookie stops working; reveal grants and the absolute
/// expiry carry over. Send the returned cookie and CSRF token to the client. Pass a transaction.
/// A session that expired meanwhile gets 401.
pub async fn reauthenticate(
    conn: &mut PgConnection,
    config: &SecurityConfig,
    current: &CurrentSession,
) -> Result<StartedSession, ApiError> {
    let (secret, remaining) = store::rotate_reauthenticated(conn, &current.id_hash())
        .await?
        .ok_or_else(ApiError::auth_required)?;
    Ok(StartedSession {
        cookie: cookie::set(
            Kind::Session,
            config.cookie_mode,
            &secret,
            Some(Duration::from_secs(remaining.ceil() as u64)),
        ),
        session: SessionDto {
            user: Some(UserDto::from(&current.user)),
            csrf_token: secret.csrf_token(CsrfPurpose::Session),
        },
    })
}

fn request_session(parts: &Parts) -> Result<&RequestSession, ApiError> {
    parts
        .extensions
        .get::<RequestSession>()
        .ok_or_else(|| ApiError::unavailable("session_layer_missing"))
}

impl<S: Send + Sync> FromRequestParts<S> for RequestSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        request_session(parts).cloned()
    }
}

/// Any live session; 401 AUTH_REQUIRED otherwise.
pub struct AuthUser(pub CurrentSession);

/// A live session whose email is verified; 403 EMAIL_UNVERIFIED otherwise (C03: catalog,
/// personal and social APIs).
pub struct VerifiedUser(pub CurrentSession);

/// A verified operator; 403 OPERATOR_REQUIRED for members.
pub struct OperatorUser(pub CurrentSession);

impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        request_session(parts)?
            .current
            .clone()
            .map(Self)
            .ok_or_else(ApiError::auth_required)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for VerifiedUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(current) = AuthUser::from_request_parts(parts, state).await?;
        if current.user.verified {
            Ok(Self(current))
        } else {
            Err(ApiError::new(ErrorCode::EmailUnverified))
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for OperatorUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let VerifiedUser(current) = VerifiedUser::from_request_parts(parts, state).await?;
        if current.user.role == Role::Operator {
            Ok(Self(current))
        } else {
            Err(ApiError::new(ErrorCode::OperatorRequired))
        }
    }
}
