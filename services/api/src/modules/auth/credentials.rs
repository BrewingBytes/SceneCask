//! Request-side helpers shared by the email/password endpoints (C04): input validation, the
//! enumeration-safe 202, C03 auth rate-limit keys, the client IP, and Argon2id hashing and
//! verification, which run on the blocking pool so they never stall the async executor.

use std::{
    net::{IpAddr, SocketAddr},
    ops::RangeInclusive,
    str::FromStr,
    sync::LazyLock,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Request},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson, rate_limit::AUTH_RULES},
};

/// C04: new passwords are 12–128 characters, never trimmed, with no composition rule.
pub(super) const PASSWORD_CHARS: RangeInclusive<usize> = 12..=128;

/// `$1` normalized as `users.normalized_email` (0001: full casefold). The result keeps the
/// column's collation so the unique index serves the lookup. The caller has already trimmed.
pub(super) const NORMALIZED: &str = r#"casefold($1 COLLATE pg_unicode_fast) COLLATE "default""#;

const CHECK_EMAIL: &str = "Check your email.";

/// `{email}`: requests that may send an email to an address.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailRequest {
    email: String,
}

/// The trimmed email of an `{email}` request, after one attempt was counted against the C03 auth
/// limits for this route, the client IP and the email. Invalid emails get 422 and are not counted.
pub(super) struct LimitedEmail(pub String);

impl FromRequest<Security> for LimitedEmail {
    type Rejection = ApiError;

    async fn from_request(request: Request, security: &Security) -> Result<Self, Self::Rejection> {
        let endpoint = request.uri().path().to_owned();
        let (mut parts, body) = request.into_parts();
        let Ok(ClientIp(ip)) = ClientIp::from_request_parts(&mut parts, security).await;
        let ApiJson(EmailRequest { email }) =
            ApiJson::from_request(Request::from_parts(parts, body), security).await?;
        let email = valid_email(&email).ok_or_else(|| {
            ApiError::new(ErrorCode::ValidationError)
                .with_field("email", "Enter a valid email address.")
        })?;
        limit_attempt(security, &endpoint, ip, email)?;
        Ok(Self(email.to_owned()))
    }
}

#[derive(Serialize)]
struct MessageDto {
    message: &'static str,
}

/// The generic 202 of every request that may send an email, whether or not the address has an
/// account.
pub(super) fn check_email() -> Response {
    (
        StatusCode::ACCEPTED,
        Json(MessageDto {
            message: CHECK_EMAIL,
        }),
    )
        .into_response()
}

/// Trimmed email that a mailbox parser accepts, or `None`. Casefolding happens in PostgreSQL.
pub(super) fn valid_email(email: &str) -> Option<&str> {
    let email = email.trim();
    (email.chars().count() <= 254 && lettre::Address::from_str(email).is_ok()).then_some(email)
}

/// The trimmed email of an `{email,password}` submission, or 422 naming each invalid field.
/// `password_error` is the password field's message when it is invalid.
pub(super) fn email_with_password<'a>(
    email: &'a str,
    password_error: Option<&'static str>,
) -> Result<&'a str, ApiError> {
    match (valid_email(email), password_error) {
        (Some(email), None) => Ok(email),
        (email, password_error) => {
            let mut error = ApiError::new(ErrorCode::ValidationError);
            if email.is_none() {
                error = error.with_field("email", "Enter a valid email address.");
            }
            if let Some(message) = password_error {
                error = error.with_field("password", message);
            }
            Err(error)
        }
    }
}

/// Counts one attempt against the C03 auth limits for `endpoint`, client IP and `identifier`.
pub(super) fn limit_attempt(
    security: &Security,
    endpoint: &str,
    ip: Option<IpAddr>,
    identifier: &str,
) -> Result<(), ApiError> {
    security
        .limiter
        .check(&limit_key(endpoint, ip, identifier), &AUTH_RULES)
}

/// C03 auth limit key: client IP plus a hash of the identifier, so no address sits in memory.
fn limit_key(endpoint: &str, ip: Option<IpAddr>, identifier: &str) -> String {
    let identifier = Sha256::digest(identifier.to_lowercase().as_bytes());
    format!(
        "{endpoint}:{}|{}",
        display_ip(ip),
        URL_SAFE_NO_PAD.encode(identifier)
    )
}

/// Limit key for an emailed-token exchange: per client IP plus token, so a shared IP (the reverse
/// proxy, NAT) is never one global budget that a client could exhaust for everyone.
pub(super) fn token_limit_key(endpoint: &str, ip: Option<IpAddr>, token: &str) -> String {
    format!(
        "{endpoint}:{}|{}",
        display_ip(ip),
        URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
    )
}

fn display_ip(ip: Option<IpAddr>) -> String {
    ip.map_or_else(|| "-".to_owned(), |ip| ip.to_string())
}

/// Argon2id (v19, default OWASP parameters) on the blocking pool, off the async executor.
pub(super) async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || hash(&password))
        .await
        .map_err(|_| ApiError::unavailable("password_hash_task"))?
        .map_err(|_| ApiError::unavailable("password_hash"))
}

fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    PasswordHasher::<PasswordHash>::hash_password(&Argon2::default(), password.as_bytes())
        .map(|hash| hash.to_string())
}

/// Stand-in for accounts without a password, so an unknown email costs one Argon2id
/// verification like a known one. `None` only if the system could not supply a salt.
static UNKNOWN_ACCOUNT: LazyLock<Option<String>> =
    LazyLock::new(|| hash("scenecask-unknown-account").ok());

/// Whether `password` matches `argon2_hash`. Without a hash this verifies against a stand-in and
/// returns false, so callers take the same time whether or not a credential exists.
pub(super) async fn verify_password(
    password: String,
    argon2_hash: Option<String>,
) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || {
        let known = argon2_hash.is_some();
        let Some(stored) = argon2_hash.or_else(|| UNKNOWN_ACCOUNT.clone()) else {
            return false;
        };
        let matches = PasswordVerifier::<str>::verify_password(
            &Argon2::default(),
            password.as_bytes(),
            &stored,
        )
        .is_ok();
        known && matches
    })
    .await
    .map_err(|_| ApiError::unavailable("password_verify_task"))
}

/// The peer address when the server is run with connect info; `None` otherwise (for example in
/// tests), in which case auth limits fall back to the identifier alone.
pub(super) struct ClientIp(pub Option<IpAddr>);

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
    fn limit_keys_hide_the_identifier() {
        let key = limit_key("register", None, "Ana@example.test");
        assert!(!key.contains("example"));
        assert_eq!(key, limit_key("register", None, "ana@example.test"));
        assert!(
            limit_key("register", Some([10, 0, 0, 1].into()), "a@b.test")
                .starts_with("register:10.0.0.1|")
        );
        let token = token_limit_key("verify", None, "secret-value");
        assert!(token.starts_with("verify:-|"));
        assert!(!token.contains("secret-value"));
    }

    #[tokio::test]
    async fn hashes_with_argon2id_and_salt() {
        let first = hash_password("a-long-test-password".into()).await.unwrap();
        let second = hash_password("a-long-test-password".into()).await.unwrap();
        assert!(first.starts_with("$argon2id$v=19$"));
        assert_ne!(first, second);
        assert!(!first.contains("a-long-test-password"));
    }

    #[tokio::test]
    async fn verifies_only_the_matching_password() {
        let stored = hash_password("a-long-test-password".into()).await.unwrap();
        let verify =
            |password: &str, stored: Option<String>| verify_password(password.into(), stored);
        assert!(
            verify("a-long-test-password", Some(stored.clone()))
                .await
                .unwrap()
        );
        assert!(
            !verify("a-long-test-passworD", Some(stored.clone()))
                .await
                .unwrap()
        );
        // Never trimmed.
        assert!(!verify(" a-long-test-password", Some(stored)).await.unwrap());
        assert!(!verify("scenecask-unknown-account", None).await.unwrap());
        assert!(!verify("x", Some("not-a-phc-string".into())).await.unwrap());
    }
}
