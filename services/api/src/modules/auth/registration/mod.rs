//! R07 email/password registration and verification (C04), relative to `/api/v1`:
//! `POST /auth/register`, `POST /auth/verify` and `POST /auth/verification-resend`.
//!
//! Register and resend always answer with the same 202 whether or not the email has an account,
//! is verified or is cooling down, and never touch SMTP: verification email goes through the
//! outbox. A pending account has no session until its link is used, so it cannot reach libraries.

mod mail;
mod store;

use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
    time::Duration,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, State},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
    routing::post,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub use mail::VerificationMail;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::session::{RequestSession, secret::Secret, start_session};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson, rate_limit::AUTH_RULES},
};

/// Outbox kind of the verification email.
pub const VERIFY_KIND: &str = "auth.verify_email";
/// C04: verification links are valid for 24 hours.
pub const TOKEN_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
/// C04: at most one verification email per minute per account.
pub const RESEND_COOLDOWN: Duration = Duration::from_secs(60);

const PASSWORD_CHARS: std::ops::RangeInclusive<usize> = 12..=128;
const CHECK_EMAIL: &str = "Check your email.";

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
struct EmailRequest {
    email: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenRequest {
    token: String,
}

#[derive(Serialize)]
struct MessageDto {
    message: &'static str,
}

fn check_email() -> Response {
    (
        StatusCode::ACCEPTED,
        Json(MessageDto {
            message: CHECK_EMAIL,
        }),
    )
        .into_response()
}

async fn register(
    State(security): State<Security>,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<RegisterRequest>,
) -> Result<Response, ApiError> {
    let password_ok = PASSWORD_CHARS.contains(&request.password.chars().count());
    let email = match valid_email(&request.email) {
        Some(email) if password_ok => email,
        email => {
            let mut error = ApiError::new(ErrorCode::ValidationError);
            if email.is_none() {
                error = error.with_field("email", "Enter a valid email address.");
            }
            if !password_ok {
                error = error.with_field("password", "Use 12–128 characters.");
            }
            return Err(error);
        }
    };
    security
        .limiter
        .check(&limit_key("register", ip, email), &AUTH_RULES)?;
    // Hashed for every request, so existing and new emails take the same time.
    let argon2_hash = hash_password(request.password).await?;
    store::register(&security.pool, email, &argon2_hash).await?;
    Ok(check_email())
}

async fn resend(
    State(security): State<Security>,
    ClientIp(ip): ClientIp,
    ApiJson(request): ApiJson<EmailRequest>,
) -> Result<Response, ApiError> {
    let email = valid_email(&request.email).ok_or_else(|| {
        ApiError::new(ErrorCode::ValidationError)
            .with_field("email", "Enter a valid email address.")
    })?;
    security
        .limiter
        .check(&limit_key("verification-resend", ip, email), &AUTH_RULES)?;
    store::resend(&security.pool, email).await?;
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
    // Per client IP plus token, so a shared IP (the reverse proxy, NAT) is never one global budget
    // that a client could exhaust for everyone.
    let key = format!(
        "verify:{}|{}",
        display_ip(ip),
        URL_SAFE_NO_PAD.encode(Sha256::digest(request.token.as_bytes()))
    );
    security.limiter.check(&key, &AUTH_RULES)?;
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

/// Trimmed email that a mailbox parser accepts, or `None`. Casefolding happens in PostgreSQL.
fn valid_email(email: &str) -> Option<&str> {
    let email = email.trim();
    (email.chars().count() <= 254 && lettre::Address::from_str(email).is_ok()).then_some(email)
}

/// C03 auth limit key: client IP plus a hash of the identifier, so no address sits in memory.
fn limit_key(endpoint: &str, ip: Option<IpAddr>, email: &str) -> String {
    let identifier = Sha256::digest(email.to_lowercase().as_bytes());
    format!(
        "{endpoint}:{}|{}",
        display_ip(ip),
        URL_SAFE_NO_PAD.encode(identifier)
    )
}

fn display_ip(ip: Option<IpAddr>) -> String {
    ip.map_or_else(|| "-".to_owned(), |ip| ip.to_string())
}

/// Argon2id (v19, default OWASP parameters) on the blocking pool, off the async executor.
async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        PasswordHasher::<PasswordHash>::hash_password(&Argon2::default(), password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|_| ApiError::unavailable("password_hash_task"))?
    .map_err(|_| ApiError::unavailable("password_hash"))
}

/// The peer address when the server is run with connect info; `None` otherwise (for example in
/// tests), in which case auth limits fall back to the identifier alone.
struct ClientIp(Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| address.ip()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_trimmed_and_must_be_a_mailbox() {
        assert_eq!(
            valid_email("  Ana@Example.test \n"),
            Some("Ana@Example.test")
        );
        assert_eq!(
            valid_email("ana+tv@example.test"),
            Some("ana+tv@example.test")
        );
        for invalid in [
            "",
            "   ",
            "ana",
            "@example.test",
            "ana@",
            "a na@example.test",
        ] {
            assert_eq!(valid_email(invalid), None, "{invalid}");
        }
        assert_eq!(
            valid_email(&format!("{}@example.test", "a".repeat(250))),
            None
        );
    }

    #[test]
    fn limit_key_hides_the_identifier() {
        let key = limit_key("register", None, "Ana@example.test");
        assert!(!key.contains("example"));
        assert_eq!(key, limit_key("register", None, "ana@example.test"));
        assert!(
            limit_key("register", Some([10, 0, 0, 1].into()), "a@b.test")
                .starts_with("register:10.0.0.1|")
        );
    }

    #[tokio::test]
    async fn hashes_with_argon2id_and_salt() {
        let first = hash_password("a-long-test-password".into()).await.unwrap();
        let second = hash_password("a-long-test-password".into()).await.unwrap();
        assert!(first.starts_with("$argon2id$v=19$"));
        assert_ne!(first, second);
        assert!(!first.contains("a-long-test-password"));
    }
}
