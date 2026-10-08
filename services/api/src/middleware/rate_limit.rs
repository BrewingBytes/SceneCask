//! In-process fixed-window rate limiter (C03 configurable defaults). State is per API process:
//! a multi-instance deployment multiplies the effective limit by the instance count.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::time::Instant;

use crate::{error::ApiError, middleware::Security, modules::auth::session::RequestSession};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub limit: u32,
    pub window: Duration,
}

impl Rule {
    pub const fn per_minute(limit: u32) -> Self {
        Self {
            limit,
            window: Duration::from_secs(60),
        }
    }

    pub const fn per_hour(limit: u32) -> Self {
        Self {
            limit,
            window: Duration::from_secs(3600),
        }
    }
}

/// C03 defaults. Auth keys combine client IP and normalized identifier.
pub const AUTH_RULES: [Rule; 2] = [Rule::per_minute(5), Rule::per_hour(20)];
pub const WRITE_RULES: [Rule; 1] = [Rule::per_minute(60)];
pub const SEARCH_RULES: [Rule; 1] = [Rule::per_minute(30)];
pub const COMMENT_RULES: [Rule; 1] = [Rule::per_minute(10)];

/// Expired windows are swept at most this often, so a check never scans the map on every call.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
struct Window {
    length: Duration,
    started: Instant,
    count: u32,
}

impl Window {
    fn expired(&self, now: Instant) -> bool {
        now >= self.started + self.length
    }
}

struct Windows {
    /// Windows per key, one per distinct rule length.
    windows: HashMap<String, Vec<Window>>,
    next_prune: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<Windows>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Windows {
                windows: HashMap::new(),
                next_prune: Instant::now() + PRUNE_INTERVAL,
            })),
        }
    }

    /// Counts one attempt for `key` against every rule. Rejected attempts are not counted, so a
    /// client that keeps retrying is not locked out beyond the current window. Callers must not
    /// put secrets in `key`; hash identifiers that may be sensitive. Memory is bounded by the keys
    /// seen within the longest window: expired windows are swept once per [`PRUNE_INTERVAL`].
    pub fn check(&self, key: &str, rules: &[Rule]) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if now >= state.next_prune {
            state.next_prune = now + PRUNE_INTERVAL;
            state
                .windows
                .retain(|_, windows| windows.iter().any(|window| !window.expired(now)));
        }
        if !state.windows.contains_key(key) {
            state.windows.insert(key.to_owned(), Vec::new());
        }
        let windows = state.windows.get_mut(key).expect("inserted above");
        let mut retry_after = None;
        for rule in rules {
            let index = match windows
                .iter()
                .position(|window| window.length == rule.window)
            {
                Some(index) => index,
                None => {
                    windows.push(Window {
                        length: rule.window,
                        started: now,
                        count: 0,
                    });
                    windows.len() - 1
                }
            };
            let window = &mut windows[index];
            if window.expired(now) {
                window.started = now;
                window.count = 0;
            }
            if window.count >= rule.limit {
                let remaining = (window.started + window.length) - now;
                let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
                retry_after = retry_after.max(Some(seconds));
            }
        }
        if let Some(seconds) = retry_after {
            return Err(ApiError::rate_limited(seconds));
        }
        for window in windows
            .iter_mut()
            .filter(|window| rules.iter().any(|rule| rule.window == window.length))
        {
            window.count += 1;
        }
        Ok(())
    }
}

/// Limits authenticated mutations per user (C03 ordinary writes). Anonymous mutations are public
/// auth endpoints, which apply [`AUTH_RULES`] per IP and identifier in their handlers.
pub async fn limit_writes(
    State(security): State<Security>,
    request: Request,
    next: Next,
) -> Response {
    if !request.method().is_safe()
        && let Some(user_id) = request
            .extensions()
            .get::<RequestSession>()
            .and_then(|session| session.current())
            .map(|current| current.user.id)
        && let Err(error) = security
            .limiter
            .check(&format!("write:{user_id}"), &[security.config.write_limit])
    {
        return error.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[tokio::test(start_paused = true)]
    async fn rejects_over_limit_until_window_resets() {
        let limiter = RateLimiter::new();
        let rules = [Rule::per_minute(2)];
        assert!(limiter.check("a", &rules).is_ok());
        assert!(limiter.check("a", &rules).is_ok());
        let error = limiter.check("a", &rules).unwrap_err();
        assert_eq!(error.code(), ErrorCode::RateLimited);
        // Separate keys have separate budgets.
        assert!(limiter.check("b", &rules).is_ok());
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(limiter.check("a", &rules).is_err());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(limiter.check("a", &rules).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn hourly_rule_outlasts_minute_rule() {
        let limiter = RateLimiter::new();
        for _ in 0..4 {
            for _ in 0..5 {
                assert!(limiter.check("ip|ana@example.test", &AUTH_RULES).is_ok());
            }
            assert!(limiter.check("ip|ana@example.test", &AUTH_RULES).is_err());
            tokio::time::advance(Duration::from_secs(60)).await;
        }
        // 20 attempts used within the hour: the minute window has reset but the hour has not.
        let error = limiter
            .check("ip|ana@example.test", &AUTH_RULES)
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::RateLimited);
    }

    #[tokio::test(start_paused = true)]
    async fn sweeps_expired_windows_on_an_interval() {
        let limiter = RateLimiter::new();
        for key in 0..1000 {
            limiter.check(&key.to_string(), &WRITE_RULES).unwrap();
        }
        tokio::time::advance(Duration::from_secs(30)).await;
        limiter.check("fresh", &WRITE_RULES).unwrap();
        // Not yet due: nothing swept even though no window has expired either.
        assert_eq!(limiter.state.lock().unwrap().windows.len(), 1001);
        tokio::time::advance(Duration::from_secs(35)).await;
        limiter.check("later", &WRITE_RULES).unwrap();
        // The first 1000 expired at 60s; "fresh" (started at 30s) is still live.
        let state = limiter.state.lock().unwrap();
        assert_eq!(state.windows.len(), 2);
        assert!(state.windows.contains_key("fresh"));
    }
}
