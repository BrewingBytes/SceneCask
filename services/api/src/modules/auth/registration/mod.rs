//! R07 email/password registration and verification (C04), relative to `/api/v1`:
//! `POST /auth/register`, `POST /auth/verify` and `POST /auth/verification-resend`.
//!
//! Register and resend always answer with the same 202 whether or not the email has an account,
//! is verified or is cooling down, and never touch SMTP: verification email goes through the
//! outbox. A pending account has no session until its link is used, so it cannot reach libraries.

mod store;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;

use super::{
    credentials::{
        ClientIp, LimitedEmail, PASSWORD_CHARS, check_email, email_with_password, hash_password,
        limit_attempt, token_limit_key,
    },
    email_token::VERIFY,
    session::{RequestSession, secret::Secret, start_session},
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson, rate_limit::AUTH_RULES},
};

/// Outbox kind of the verification email.
pub const VERIFY_KIND: &str = VERIFY.kind;
/// C04: verification links are valid for 24 hours.
pub const TOKEN_LIFETIME: Duration = VERIFY.lifetime;
/// C04: at most one verification email per minute per account.
pub const RESEND_COOLDOWN: Duration = VERIFY.cooldown;

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/verify", post(verify))
        .route("/auth/verification-resend", post(resend))
        .with_state(security)
}

/// Credentials are write-only: no `Debug`, so they cannot reach a log by accident.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenRequest {
    token: String,
}

async fn register(
    State(security): State<Security>,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<RegisterRequest>,
) -> Result<Response, ApiError> {
    let password_ok = PASSWORD_CHARS.contains(&request.password.chars().count());
    let email = email_with_password(
        &request.email,
        (!password_ok).then_some("Use 12–128 characters."),
    )?;
    limit_attempt(&security, "register", ip, email)?;
    // Hashed for every request, so existing and new emails take the same time.
    let argon2_hash = hash_password(request.password).await?;
    store::register(&security.pool, email, &argon2_hash).await?;
    Ok(check_email())
}

async fn resend(
    State(security): State<Security>,
    LimitedEmail(email): LimitedEmail,
) -> Result<Response, ApiError> {
    store::resend(&security.pool, &email).await?;
    Ok(check_email())
}

/// Consumes the link, marks the account verified and signs it in, rotating away any session the
/// request carried. Unknown, used, invalidated and expired links all get 410 TOKEN_EXPIRED, whose
/// recovery is a resend.
async fn verify(
    State(security): State<Security>,
    session: RequestSession,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<TokenRequest>,
) -> Result<Response, ApiError> {
    if request.token.is_empty() {
        return Err(ApiError::new(ErrorCode::ValidationError)
            .with_field("token", "Use the link from your email."));
    }
    security
        .limiter
        .check(&token_limit_key("verify", ip, &request.token), &AUTH_RULES)?;
    let expired = || ApiError::new(ErrorCode::TokenExpired);
    let secret = Secret::decode(&request.token).ok_or_else(expired)?;
    let mut tx = security.pool.begin().await?;
    let user_id = store::consume_token(&mut tx, &secret.hash())
        .await?
        .ok_or_else(expired)?;
    let started = start_session(&mut tx, &security.config, session.current(), user_id).await?;
    tx.commit().await?;
    Ok((
        [(header::SET_COOKIE, started.cookie)],
        Json(started.session),
    )
        .into_response())
}
