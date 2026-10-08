//! C03/C04 request security layers. Integration owners (R24/R38) mount the API router through
//! [`apply`]; this module does not register routes on the shared router.

pub mod csrf;
pub mod json;
pub mod rate_limit;
pub mod request;

use std::sync::Arc;

use axum::{
    Router, extract::DefaultBodyLimit, middleware::from_fn, middleware::from_fn_with_state,
};
use rate_limit::{RateLimiter, Rule};
use sqlx::PgPool;

use crate::{
    error,
    modules::auth::session::{self, Lifetimes},
};

/// Cookie attributes. The non-Secure development exception exists only for a loopback origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieMode {
    /// `__Host-` prefixed, Secure, HttpOnly, SameSite=Lax, Path=/.
    Secure,
    /// Development on http://localhost or loopback IPs: same attributes without Secure or prefix.
    LocalhostDevelopment,
}

#[derive(Clone, Debug)]
pub struct SecurityConfig {
    /// Serialized public origin (`scheme://host[:port]`) that mutations must send as `Origin`.
    pub public_origin: String,
    pub cookie_mode: CookieMode,
    pub lifetimes: Lifetimes,
    pub body_limit_bytes: usize,
    pub write_limit: Rule,
}

impl SecurityConfig {
    /// Builds the C03/C04 defaults for `public_origin`. HTTPS origins get Secure cookies; plain
    /// HTTP is accepted only for localhost or loopback. Errors never echo the value.
    pub fn new(public_origin: &str) -> Result<Self, &'static str> {
        const INVALID: &str = "API_ORIGIN must be an https origin, or http on localhost";
        let origin = public_origin.trim().trim_end_matches('/');
        let (scheme, authority) = origin.split_once("://").ok_or(INVALID)?;
        if authority.is_empty()
            || authority.contains(['/', '?', '#', '@', ' '])
            || origin
                .chars()
                .any(|c| c.is_ascii_uppercase() || c.is_control())
        {
            return Err(INVALID);
        }
        // Split an optional ":port" after the host (an IPv6 host keeps its brackets).
        let split = match authority.find(']') {
            Some(end) => end + 1,
            None => authority.find(':').unwrap_or(authority.len()),
        };
        let (host, port) = authority.split_at(split);
        let port = match port.strip_prefix(':') {
            // Canonical decimal only, so the stored origin matches what browsers serialize.
            Some(text) => match text.parse::<u16>() {
                Ok(number) if number != 0 && number.to_string() == text => Some(number),
                _ => return Err(INVALID),
            },
            None if port.is_empty() && !host.is_empty() => None,
            None => return Err(INVALID),
        };
        let cookie_mode = match scheme {
            "https" => CookieMode::Secure,
            "http" if matches!(host, "localhost" | "127.0.0.1" | "[::1]") => {
                CookieMode::LocalhostDevelopment
            }
            _ => return Err(INVALID),
        };
        // Browsers omit the scheme's default port from Origin, so drop it here too.
        let default_port = if scheme == "https" { 443 } else { 80 };
        let public_origin = match port {
            Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
            _ => format!("{scheme}://{host}"),
        };
        Ok(Self {
            public_origin,
            cookie_mode,
            lifetimes: Lifetimes::default(),
            body_limit_bytes: 64 * 1024,
            write_limit: rate_limit::WRITE_RULES[0],
        })
    }
}

/// Shared state for the security layers and session routes.
#[derive(Clone)]
pub struct Security {
    pub pool: PgPool,
    pub config: Arc<SecurityConfig>,
    pub limiter: RateLimiter,
}

impl Security {
    pub fn new(pool: PgPool, config: SecurityConfig) -> Self {
        Self {
            pool,
            config: Arc::new(config),
            limiter: RateLimiter::new(),
        }
    }
}

/// Wraps an `/api/v1` router with the C03 layers. Outermost first: request ID and error envelope
/// scope, private no-store headers, body limits, session identity, origin/CSRF/content-type
/// checks, then the per-user write limit. Unknown paths and wrong methods on known paths get a
/// 404 NOT_FOUND envelope (C03 defines no 405 code).
pub fn apply(router: Router, security: &Security) -> Router {
    router
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::not_found)
        .layer(from_fn_with_state(
            security.clone(),
            rate_limit::limit_writes,
        ))
        .layer(from_fn_with_state(security.clone(), csrf::protect))
        .layer(from_fn_with_state(security.clone(), session::load))
        .layer(DefaultBodyLimit::max(security.config.body_limit_bytes))
        .layer(from_fn_with_state(security.clone(), request::limit_body))
        .layer(from_fn(request::private_headers))
        .layer(from_fn(request::request_id))
}

/// The session endpoints (`GET /session`, `POST /auth/logout`) with every layer applied, ready to
/// merge or nest under `/api/v1` during integration.
pub fn session_api(security: &Security) -> Router {
    apply(session::routes(security.clone()), security)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_selects_cookie_mode_and_rejects_unsafe_values() {
        let secure = SecurityConfig::new("https://scenecask.example/").unwrap();
        assert_eq!(secure.public_origin, "https://scenecask.example");
        assert_eq!(secure.cookie_mode, CookieMode::Secure);
        for (configured, serialized) in [
            ("https://scenecask.example:443", "https://scenecask.example"),
            (
                "https://scenecask.example:8443",
                "https://scenecask.example:8443",
            ),
            ("http://localhost:80", "http://localhost"),
            ("http://[::1]:3000", "http://[::1]:3000"),
        ] {
            assert_eq!(
                SecurityConfig::new(configured).unwrap().public_origin,
                serialized
            );
        }
        for local in [
            "http://localhost:3000",
            "http://127.0.0.1:8080",
            "http://[::1]:3000",
        ] {
            assert_eq!(
                SecurityConfig::new(local).unwrap().cookie_mode,
                CookieMode::LocalhostDevelopment
            );
        }
        for invalid in [
            "http://scenecask.example",
            "https://scenecask.example/path",
            "https://user@scenecask.example",
            "https://Scenecask.example",
            "https://scenecask.example:",
            "https://scenecask.example:0443",
            "https://scenecask.example:0",
            "https://scenecask.example:65536",
            "ftp://localhost",
            "https://",
            "localhost:3000",
        ] {
            assert!(SecurityConfig::new(invalid).is_err(), "{invalid}");
        }
    }
}
