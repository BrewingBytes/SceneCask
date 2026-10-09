//! `oauth_flows` (0001): one single-use row per Google redirect, keyed by the state's hash and
//! expiring after ten minutes. The nonce is stored only as a hash and the PKCE verifier only as
//! ciphertext under a key derived from the state, which the database never sees; the state itself
//! travels in the URL and in a browser-binding cookie. Also validates `returnTo`.

use std::time::Duration;

use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use hmac::{Hmac, Mac};
use openidconnect::url::Url;
use sha2::Sha256;
use sqlx::{PgPool, Row};

use crate::{error::ApiError, modules::auth::session::secret::Secret};

/// C04: single-use state/nonce/PKCE lifetime.
pub const FLOW_LIFETIME: Duration = Duration::from_secs(10 * 60);

pub const DEFAULT_RETURN_TO: &str = "/home";

/// Application routes a flow may return to: each entry matches itself and any path below it.
/// Auth pages are excluded so a flow never lands on a token-bearing or sign-in screen.
const RETURN_ROUTES: [&str; 9] = [
    "/home",
    "/discover",
    "/library",
    "/shows",
    "/episodes",
    "/settings",
    "/onboarding/profile",
    "/friends",
    "/notifications",
];

const MAX_RETURN_TO: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Signin,
    Link,
    Reauth,
}

impl Intent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Signin => "signin",
            Self::Link => "link",
            Self::Reauth => "reauth",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "signin" => Some(Self::Signin),
            "link" => Some(Self::Link),
            "reauth" => Some(Self::Reauth),
            _ => None,
        }
    }
}

/// The once-decoded `returnTo` query value as a normalized same-origin application path
/// (path and query, fragment dropped), or `None` if it is not allowlisted. The value is never
/// decoded again: encoded controls, spaces, separators and percent signs are rejected outright.
pub fn return_to(value: &str, public_origin: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let syntax_ok = value.len() <= MAX_RETURN_TO
        && bytes.first() == Some(&b'/')
        && !matches!(bytes.get(1), Some(b'/' | b'\\'))
        && bytes
            .iter()
            .all(|&byte| (0x21..=0x7e).contains(&byte) && byte != b'\\')
        && percent_escapes_safe(bytes);
    if !syntax_ok {
        return None;
    }
    let origin = Url::parse(public_origin).ok()?;
    let resolved = origin.join(value).ok()?;
    if resolved.origin() != origin.origin() {
        return None;
    }
    let path = resolved.path();
    let allowed = RETURN_ROUTES.iter().any(|route| {
        path.strip_prefix(route)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    });
    allowed.then(|| match resolved.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    })
}

/// Every `%` starts a two-digit escape that is not a control, space, `/`, `\`, `%` or DEL.
fn percent_escapes_safe(bytes: &[u8]) -> bool {
    let mut index = 0;
    while let Some(offset) = bytes[index..].iter().position(|&byte| byte == b'%') {
        let start = index + offset + 1;
        let Some(decoded) = bytes
            .get(start..start + 2)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        else {
            return false;
        };
        if decoded <= 0x20 || matches!(decoded, b'/' | b'\\' | b'%' | 0x7f) {
            return false;
        }
        index = start + 2;
    }
    true
}

/// The secrets of a new flow. `state` and `nonce` go to Google; only their hashes are stored.
pub struct NewFlow {
    pub state: Secret,
    pub nonce: Secret,
    pub verifier: String,
}

/// ChaCha20-Poly1305 keyed by HMAC-SHA256 of the flow's state. Each key encrypts exactly one
/// message, so the fixed nonce is never reused under a key.
fn cipher(state: &Secret) -> ChaCha20Poly1305 {
    let mut mac = Hmac::<Sha256>::new_from_slice(state.encode().as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(b"scenecask/google/pkce/v1");
    ChaCha20Poly1305::new_from_slice(&mac.finalize().into_bytes()).expect("32-byte key")
}

fn encrypt(state: &Secret, verifier: &str) -> Result<Vec<u8>, ApiError> {
    cipher(state)
        .encrypt(&Nonce::default(), verifier.as_bytes())
        .map_err(|_| ApiError::unavailable("pkce_encrypt"))
}

/// The PKCE verifier, or `None` if the ciphertext was not sealed under this state.
pub fn decrypt(state: &Secret, ciphertext: &[u8]) -> Option<String> {
    let plain = cipher(state).decrypt(&Nonce::default(), ciphertext).ok()?;
    String::from_utf8(plain).ok()
}

/// Stores a flow. Link and reauth flows belong to `session_hash`, so revoking or rotating that
/// session deletes them (0001 cascade).
pub async fn insert(
    pool: &PgPool,
    flow: &NewFlow,
    intent: Intent,
    session_hash: Option<&[u8]>,
    return_to: &str,
) -> Result<(), ApiError> {
    let ciphertext = encrypt(&flow.state, &flow.verifier)?;
    sqlx::query(
        "INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent,
                                  session_id, return_to, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7))",
    )
    .bind(flow.state.hash())
    .bind(flow.nonce.hash())
    .bind(ciphertext)
    .bind(intent.as_str())
    .bind(session_hash)
    .bind(return_to)
    .bind(FLOW_LIFETIME.as_secs_f64())
    .execute(pool)
    .await?;
    Ok(())
}

/// A flow found by its state.
pub struct StoredFlow {
    pub intent: Intent,
    pub return_to: String,
    pub session_hash: Option<Vec<u8>>,
    pub nonce_hash: Vec<u8>,
    pub ciphertext: Vec<u8>,
    /// True only for the one request that consumed a live flow; replays and expired flows are
    /// false.
    pub consumed_now: bool,
}

/// Marks the flow for `state_hash` consumed if it is unused and unexpired, in one statement, so
/// at most one callback can ever use a flow. Commits independently of what follows: a failed
/// exchange still spends the flow.
pub async fn consume(pool: &PgPool, state_hash: &[u8]) -> Result<Option<StoredFlow>, ApiError> {
    let row = sqlx::query(
        "WITH flow AS (
             SELECT * FROM oauth_flows WHERE state_hash = $1 FOR UPDATE
         ), used AS (
             UPDATE oauth_flows o SET consumed_at = now() FROM flow
             WHERE o.state_hash = flow.state_hash
               AND flow.consumed_at IS NULL AND flow.expires_at > now()
             RETURNING o.state_hash
         )
         SELECT flow.intent, flow.return_to, flow.session_id, flow.nonce_hash,
                flow.pkce_verifier_ciphertext, EXISTS (SELECT 1 FROM used) AS consumed_now
         FROM flow",
    )
    .bind(state_hash)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let intent: String = row.try_get("intent")?;
    Ok(Some(StoredFlow {
        intent: Intent::parse(&intent).ok_or_else(|| ApiError::unavailable("flow_intent"))?,
        return_to: row.try_get("return_to")?,
        session_hash: row.try_get("session_id")?,
        nonce_hash: row.try_get("nonce_hash")?,
        ciphertext: row.try_get("pkce_verifier_ciphertext")?,
        consumed_now: row.try_get("consumed_now")?,
    }))
}

/// Deletes flows that expired more than a day ago; for a periodic worker. Consumed rows are kept
/// until then so replays still resolve to their intent's error route.
pub async fn purge_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query("DELETE FROM oauth_flows WHERE expires_at <= now() - interval '1 day'")
        .execute(pool)
        .await
        .map(|result| result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://scenecask.example";

    #[test]
    fn return_to_accepts_allowlisted_application_paths() {
        for (value, normalized) in [
            ("/home", "/home"),
            ("/settings", "/settings"),
            ("/settings/data", "/settings/data"),
            (
                "/discover?q=hollow%2Borchard",
                "/discover?q=hollow%2Borchard",
            ),
            ("/shows/1b9d6bcd/../2c8e", "/shows/2c8e"),
            ("/library?status=watching#top", "/library?status=watching"),
            ("/onboarding/profile", "/onboarding/profile"),
        ] {
            assert_eq!(
                return_to(value, ORIGIN).as_deref(),
                Some(normalized),
                "{value}"
            );
        }
    }

    #[test]
    fn return_to_rejects_other_origins_and_unsafe_encodings() {
        for value in [
            "",
            "home",
            "//evil.example/home",
            "/\\evil.example",
            "https://evil.example/home",
            "/home\\x",
            "/home x",
            "/home\t",
            "/homepage",
            "/auth/verify",
            "/api/v1/session",
            "/shows/../auth/signin",
            "/%2e%2e/auth",
            "/home%0d%0aSet-Cookie:x",
            "/home%20",
            "/home%2f..%2fauth",
            "/home%5c",
            "/home%25",
            "/home%7f",
            "/home%",
            "/home%zz",
            "/hôme",
        ] {
            assert_eq!(return_to(value, ORIGIN), None, "{value:?}");
        }
        assert_eq!(
            return_to(&format!("/home?{}", "a".repeat(600)), ORIGIN),
            None
        );
    }

    #[test]
    fn verifier_round_trips_only_under_its_state() {
        let state = Secret::generate().unwrap();
        let ciphertext = encrypt(&state, "pkce-verifier-value").unwrap();
        assert!(
            !ciphertext
                .windows(b"pkce-verifier-value".len())
                .any(|window| window == b"pkce-verifier-value")
        );
        assert_eq!(
            decrypt(&state, &ciphertext).as_deref(),
            Some("pkce-verifier-value")
        );
        assert_eq!(decrypt(&Secret::generate().unwrap(), &ciphertext), None);
        let mut tampered = ciphertext.clone();
        tampered[0] ^= 1;
        assert_eq!(decrypt(&state, &tampered), None);
    }

    #[test]
    fn intents_round_trip() {
        for intent in [Intent::Signin, Intent::Link, Intent::Reauth] {
            assert_eq!(Intent::parse(intent.as_str()), Some(intent));
        }
        assert_eq!(Intent::parse("merge"), None);
    }
}
