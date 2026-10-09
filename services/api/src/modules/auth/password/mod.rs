//! R08 password login, recovery and fresh reauthentication (C04), relative to `/api/v1`:
//! `POST /auth/login`, `POST /auth/password-reset-request`, `POST /auth/password-reset` and
//! `POST /auth/reauth`.
//!
//! Login and reauth answer every wrong email, password, missing credential or disabled account
//! with the same 401 INVALID_CREDENTIALS after one Argon2id verification, so neither the answer
//! nor its timing says whether an account exists. Only the correct password of an unverified
//! account gets 403 EMAIL_UNVERIFIED, and never a session. Reset requests always answer the same
//! 202 and send through the outbox ([`super::email_token::RESET`]).

mod store;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;

use super::{
    credentials::{
        ClientIp, LimitedEmail, PASSWORD_CHARS, check_email, email_with_password, hash_password,
        limit_attempt, token_limit_key, verify_password,
    },
    session::{
        AuthUser, RequestSession, cookie, cookie::Kind, reauthenticate, secret::Secret,
        start_session,
    },
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson, rate_limit::AUTH_RULES},
};

/// C04: link, unlink and account deletion require a sign-in or reauthentication this recent
/// ([`super::session::CurrentSession::reauthenticated_within`]).
pub const REAUTH_WINDOW: Duration = Duration::from_secs(5 * 60);

const ENTER_PASSWORD: &str = "Enter your password.";

/// A submitted (not new) password: anything up to the longest password that can be set.
const SUBMITTED_CHARS: std::ops::RangeInclusive<usize> = 1..=*PASSWORD_CHARS.end();

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/password-reset-request", post(request_reset))
        .route("/auth/password-reset", post(reset))
        .route("/auth/reauth", post(reauth))
        .with_state(security)
}

/// Credentials are write-only: no `Debug`, so they cannot reach a log by accident.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ResetRequest {
    token: String,
    new_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReauthRequest {
    password: String,
}

fn invalid_credentials() -> ApiError {
    ApiError::new(ErrorCode::InvalidCredentials)
}

fn submitted_password_ok(password: &str) -> bool {
    SUBMITTED_CHARS.contains(&password.chars().count())
}

/// Signs in with a verified account's password, rotating away any session the request carried.
async fn login(
    State(security): State<Security>,
    session: RequestSession,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    let email = email_with_password(
        &request.email,
        (!submitted_password_ok(&request.password)).then_some(ENTER_PASSWORD),
    )?;
    limit_attempt(&security, "login", ip, email)?;
    let account = store::login_account(&security.pool, email).await?;
    let stored = account.as_ref().map(|account| account.argon2_hash.clone());
    if !verify_password(request.password, stored).await? {
        return Err(invalid_credentials());
    }
    let account = account.ok_or_else(invalid_credentials)?;
    if !account.verified {
        return Err(ApiError::new(ErrorCode::EmailUnverified));
    }
    let mut tx = security.pool.begin().await?;
    let started = start_session(&mut tx, &security.config, session.current(), account.id).await?;
    tx.commit().await?;
    Ok((
        [(header::SET_COOKIE, started.cookie)],
        Json(started.session),
    )
        .into_response())
}

async fn request_reset(
    State(security): State<Security>,
    LimitedEmail(email): LimitedEmail,
) -> Result<Response, ApiError> {
    store::request_reset(&security.pool, &email).await?;
    Ok(check_email())
}

/// Consumes the reset link, sets the new password and signs the account out everywhere. Unknown,
/// used, superseded and expired links all get 410 TOKEN_EXPIRED, whose recovery is a new request.
async fn reset(
    State(security): State<Security>,
    session: RequestSession,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<ResetRequest>,
) -> Result<Response, ApiError> {
    let token_ok = !request.token.is_empty();
    let password_ok = PASSWORD_CHARS.contains(&request.new_password.chars().count());
    if !token_ok || !password_ok {
        let mut error = ApiError::new(ErrorCode::ValidationError);
        if !token_ok {
            error = error.with_field("token", "Use the link from your email.");
        }
        if !password_ok {
            error = error.with_field("newPassword", "Use 12–128 characters.");
        }
        return Err(error);
    }
    security.limiter.check(
        &token_limit_key("password-reset", ip, &request.token),
        &AUTH_RULES,
    )?;
    let expired = || ApiError::new(ErrorCode::TokenExpired);
    let secret = Secret::decode(&request.token).ok_or_else(expired)?;
    // Hashed before the transaction so the token row is never locked for an Argon2id run.
    let argon2_hash = hash_password(request.new_password).await?;
    let mut tx = security.pool.begin().await?;
    let user_id = store::reset(&mut tx, &secret.hash(), &argon2_hash)
        .await?
        .ok_or_else(expired)?;
    tx.commit().await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    // The request's own session was revoked with the others; stop the browser presenting it.
    if session
        .current()
        .is_some_and(|current| current.user.id == user_id)
    {
        response.headers_mut().append(
            header::SET_COOKIE,
            cookie::clear(Kind::Session, security.config.cookie_mode),
        );
    }
    Ok(response)
}

/// Confirms the signed-in user's password and rotates the session as freshly reauthenticated.
/// The rotated cookie comes with a new CSRF token, which the client reads from `GET /session`.
/// Accounts without a password (Google only) get the same 401 and use the Google reauth intent.
async fn reauth(
    State(security): State<Security>,
    AuthUser(current): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<ReauthRequest>,
) -> Result<Response, ApiError> {
    if !submitted_password_ok(&request.password) {
        return Err(
            ApiError::new(ErrorCode::ValidationError).with_field("password", ENTER_PASSWORD)
        );
    }
    limit_attempt(&security, "reauth", ip, &current.user.id.to_string())?;
    let stored = store::password_hash(&security.pool, current.user.id).await?;
    if !verify_password(request.password, stored).await? {
        return Err(invalid_credentials());
    }
    let mut tx = security.pool.begin().await?;
    let started = reauthenticate(&mut tx, &security.config, &current).await?;
    tx.commit().await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, started.cookie)],
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_passwords_are_counted_in_characters() {
        assert!(!submitted_password_ok(""));
        assert!(submitted_password_ok("x"));
        assert!(submitted_password_ok(&"é".repeat(128)));
        assert!(!submitted_password_ok(&"é".repeat(129)));
    }
}
