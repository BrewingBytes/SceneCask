//! R09 Google sign-in and explicit identity linking (C04), relative to `/api/v1`:
//! `GET /auth/google/start` and `GET /auth/google/callback`.
//!
//! Start stores a single-use flow ([`flow`]) and redirects to Google with state, nonce and an
//! S256 PKCE challenge; the state is also set in a short-lived cookie so only the browser that
//! started a flow can finish it. The callback spends the flow before anything else, exchanges the
//! code and validates the ID token ([`provider`]), then acts on the flow's intent:
//!
//! - `signin`: the account owning `(issuer, subject)` signs in. An unknown subject with a verified
//!   email creates a private account, unless any account uses that email: that needs explicit
//!   linking from the signed-in account, never a merge.
//! - `link`: adds the identity to the session that started the flow (fresh within
//!   [`REAUTH_WINDOW`] at start).
//! - `reauth`: marks that session freshly reauthenticated if the identity is its own and Google
//!   authenticated the user again for this flow (`auth_time`), not from an existing Google session.
//!
//! The callback always answers 303 to an application route, with `?error=<code>` on failure, and
//! changes no account, identity or session unless it succeeds. Google tokens are never stored.

mod flow;
mod provider;
mod store;

use std::{net::IpAddr, sync::Arc};

use axum::{
    Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use flow::{DEFAULT_RETURN_TO, NewFlow, StoredFlow};
pub use flow::{FLOW_LIFETIME, Intent, purge_expired};
use openidconnect::PkceCodeChallenge;
use provider::VerifiedIdentity;
pub use provider::{GOOGLE_ISSUER, Google, GoogleConfig, ProviderError};
use serde::Deserialize;
use sqlx::PgConnection;
pub(super) use store::lock_account;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use super::{
    credentials::{ClientIp, valid_email},
    password::REAUTH_WINDOW,
    session::{
        CurrentSession, RequestSession,
        cookie::{self, Kind},
        reauthenticate,
        secret::Secret,
        start_session,
    },
};
use crate::{
    error::{ApiError, ErrorCode},
    middleware::{CookieMode, Security, rate_limit::WRITE_RULES},
};

const ONBOARDING: &str = "/onboarding/profile";
const SIGNIN_PAGE: &str = "/auth/signin";

#[derive(Clone)]
struct GoogleState {
    security: Security,
    google: Arc<Google>,
}

pub fn routes(security: Security, google: Arc<Google>) -> Router {
    Router::new()
        .route("/auth/google/start", get(start))
        .route("/auth/google/callback", get(callback))
        .with_state(GoogleState { security, google })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartQuery {
    intent: Option<String>,
    return_to: Option<String>,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Starts a flow and redirects to Google. Signin is public; link needs a session reauthenticated
/// within [`REAUTH_WINDOW`] and no Google identity yet; reauth needs a session whose account has
/// one. A stale session gets 401 AUTH_REQUIRED, the cue to reauthenticate first.
async fn start(
    State(GoogleState { security, google }): State<GoogleState>,
    session: RequestSession,
    ClientIp(ip): ClientIp,
    query: Result<Query<StartQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::malformed())?;
    let intent = query.intent.as_deref().and_then(Intent::parse);
    let return_to = match query.return_to.as_deref() {
        None => Some(DEFAULT_RETURN_TO.to_owned()),
        Some(value) => flow::return_to(value, &security.config.public_origin),
    };
    let (intent, return_to) = match (intent, return_to) {
        (Some(intent), Some(return_to)) => (intent, return_to),
        (intent, return_to) => {
            let mut error = ApiError::new(ErrorCode::ValidationError);
            if intent.is_none() {
                error = error.with_field("intent", "Choose signin, link or reauth.");
            }
            if return_to.is_none() {
                error = error.with_field("returnTo", "Use an application path.");
            }
            return Err(error);
        }
    };
    limit(&security, "google-start", ip)?;
    let session_hash = match intent {
        Intent::Signin => None,
        Intent::Link | Intent::Reauth => {
            let current = session.current().ok_or_else(ApiError::auth_required)?;
            let linked = has_identity(&security, current.user.id, google.issuer()).await?;
            match intent {
                Intent::Link if !current.reauthenticated_within(REAUTH_WINDOW) => {
                    return Err(ApiError::auth_required());
                }
                Intent::Link if linked => return Err(ApiError::new(ErrorCode::IdentityInUse)),
                Intent::Reauth if !linked => {
                    return Err(ApiError::new(ErrorCode::IdentityLinkRequired));
                }
                _ => Some(current.id_hash()),
            }
        }
    };
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let new = NewFlow {
        state: Secret::generate()?,
        nonce: Secret::generate()?,
        verifier: verifier.into_secret(),
    };
    let url = google
        .authorize_url(
            new.state.encode(),
            new.nonce.encode(),
            challenge,
            intent == Intent::Reauth,
        )
        .await
        .map_err(|error| ApiError::unavailable(error.category()))?;
    flow::insert(
        &security.pool,
        &new,
        intent,
        session_hash.as_deref(),
        &return_to,
    )
    .await?;
    let binding = cookie::set(
        Kind::GoogleFlow,
        security.config.cookie_mode,
        &new.state,
        Some(FLOW_LIFETIME),
    );
    redirect(url.as_str(), [binding])
}

async fn has_identity(security: &Security, user_id: Uuid, issuer: &str) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM external_identities WHERE user_id = $1 AND issuer = $2)",
    )
    .bind(user_id)
    .bind(issuer)
    .fetch_one(&security.pool)
    .await?)
}

/// Why a callback failed: the `error` query value of the route it redirects to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The user declined at Google.
    Canceled,
    /// Unknown, used or expired flow.
    Expired,
    /// Wrong browser or session, missing code, or a code or ID token that failed validation.
    Failed,
    /// Google could not be reached.
    Unavailable,
    /// A new account needs a Google-verified email.
    EmailUnverified,
    /// An account already uses this email: sign in with it, then link Google in settings.
    LinkRequired,
    /// The identity belongs to another account.
    IdentityInUse,
    /// The Google account is not the one linked to the signed-in account.
    ReauthMismatch,
}

impl Failure {
    /// The failure for an authorization `error` from Google: only `access_denied` is the user
    /// declining; Google's own outages are `unavailable` and anything else (misconfiguration,
    /// unexpected values) is `failed`.
    fn from_provider(error: &str) -> Self {
        match error {
            "access_denied" => Self::Canceled,
            "temporarily_unavailable" | "server_error" => Self::Unavailable,
            _ => Self::Failed,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Canceled => "canceled",
            Self::Expired => "expired",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
            Self::EmailUnverified => "email_unverified",
            Self::LinkRequired => "link_required",
            Self::IdentityInUse => "identity_in_use",
            Self::ReauthMismatch => "reauth_mismatch",
        }
    }
}

/// Where a callback lands, with an optional rotated session cookie.
struct Done {
    location: String,
    session_cookie: Option<HeaderValue>,
}

/// Completes the flow named by `state` and redirects. The binding cookie is always cleared.
/// Failures redirect sign-in to the sign-in page, and link or reauth back to their `returnTo`.
async fn callback(
    State(GoogleState { security, google }): State<GoogleState>,
    session: RequestSession,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::malformed())?;
    let Some(state) = query.state.filter(|state| !state.is_empty()) else {
        return Err(ApiError::new(ErrorCode::ValidationError)
            .with_field("state", "Start Google sign-in again."));
    };
    limit(&security, "google-callback", ip)?;
    let mode = security.config.cookie_mode;
    let state = Secret::decode(&state);
    let flow = match &state {
        Some(state) => flow::consume(&security.pool, &state.hash()).await?,
        None => None,
    };
    let (Some(state), Some(flow)) = (state, flow) else {
        return failed(mode, None, Failure::Expired);
    };
    let callback = Callback {
        security: &security,
        google: &google,
        session: &session,
        headers: &headers,
        state: &state,
        flow: &flow,
    };
    match callback
        .complete(
            query.code,
            query.error.as_deref().map(Failure::from_provider),
        )
        .await?
    {
        Ok(done) => {
            let clear = cookie::clear(Kind::GoogleFlow, mode);
            redirect(
                &done.location,
                [Some(clear), done.session_cookie].into_iter().flatten(),
            )
        }
        Err(failure) => failed(mode, Some(&flow), failure),
    }
}

struct Callback<'a> {
    security: &'a Security,
    google: &'a Google,
    session: &'a RequestSession,
    headers: &'a HeaderMap,
    state: &'a Secret,
    flow: &'a StoredFlow,
}

type Outcome = Result<Result<Done, Failure>, ApiError>;

impl Callback<'_> {
    /// `provider_error` is the failure Google reported instead of a code, if any.
    async fn complete(&self, code: Option<String>, provider_error: Option<Failure>) -> Outcome {
        if !self.flow.consumed_now {
            return Ok(Err(Failure::Expired));
        }
        let mode = self.security.config.cookie_mode;
        let bound = cookie::read(self.headers, Kind::GoogleFlow, mode)
            .is_some_and(|cookie| bool::from(cookie.hash().ct_eq(&self.state.hash())));
        if !bound {
            return Ok(Err(Failure::Failed));
        }
        if let Some(failure) = provider_error {
            return Ok(Err(failure));
        }
        let Some(code) = code.filter(|code| !code.is_empty()) else {
            return Ok(Err(Failure::Failed));
        };
        // Link and reauth finish only in the session that started them.
        let current = match (self.flow.intent, self.session.current()) {
            (Intent::Signin, _) => None,
            (_, Some(current)) if self.flow.session_hash.as_ref() == Some(&current.id_hash()) => {
                Some(current)
            }
            _ => return Ok(Err(Failure::Failed)),
        };
        let Some(verifier) = flow::decrypt(self.state, &self.flow.ciphertext) else {
            return Ok(Err(Failure::Failed));
        };
        let identity = match self
            .google
            .exchange(
                code,
                verifier,
                &self.flow.nonce_hash,
                self.flow.intent == Intent::Reauth,
            )
            .await
        {
            Ok(identity) => identity,
            Err(error) => {
                tracing::warn!(category = error.category(), "google callback rejected");
                return Ok(Err(match error {
                    ProviderError::Unavailable => Failure::Unavailable,
                    ProviderError::Rejected => Failure::Failed,
                }));
            }
        };
        match (self.flow.intent, current) {
            (Intent::Link, Some(current)) => self.link(current, &identity).await,
            (Intent::Reauth, Some(current)) => self.reauth(current, &identity).await,
            _ => self.signin(&identity).await,
        }
    }

    async fn signin(&self, identity: &VerifiedIdentity) -> Outcome {
        let mut tx = self.security.pool.begin().await?;
        store::lock_identity(&mut tx, &identity.issuer, &identity.subject).await?;
        let (user_id, created) =
            match store::owner(&mut tx, &identity.issuer, &identity.subject).await? {
                Some(user_id) => (user_id, false),
                None => match new_account(&mut tx, identity).await? {
                    Ok(user_id) => (user_id, true),
                    Err(failure) => return Ok(Err(failure)),
                },
            };
        let started = match start_session(
            &mut tx,
            &self.security.config,
            self.session.current(),
            user_id,
        )
        .await
        {
            Ok(started) => started,
            Err(error) if error.code() == ErrorCode::ServiceUnavailable => return Err(error),
            // Disabled meanwhile.
            Err(_) => return Ok(Err(Failure::Failed)),
        };
        tx.commit().await?;
        Ok(Ok(Done {
            location: if created {
                ONBOARDING.to_owned()
            } else {
                self.flow.return_to.clone()
            },
            session_cookie: Some(started.cookie),
        }))
    }

    async fn link(&self, current: &CurrentSession, identity: &VerifiedIdentity) -> Outcome {
        let mut tx = self.security.pool.begin().await?;
        match store::link(
            &mut tx,
            current.user.id,
            &identity.issuer,
            &identity.subject,
        )
        .await?
        {
            store::Link::Linked | store::Link::AlreadyLinked => {}
            store::Link::InUse => return Ok(Err(Failure::IdentityInUse)),
        }
        tx.commit().await?;
        Ok(Ok(self.done(None)))
    }

    async fn reauth(&self, current: &CurrentSession, identity: &VerifiedIdentity) -> Outcome {
        let mut tx = self.security.pool.begin().await?;
        store::lock_account(&mut tx, current.user.id).await?;
        let owner = store::owner(&mut tx, &identity.issuer, &identity.subject).await?;
        if owner != Some(current.user.id) {
            return Ok(Err(Failure::ReauthMismatch));
        }
        let started = match reauthenticate(&mut tx, &self.security.config, current).await {
            Ok(started) => started,
            Err(error) if error.code() == ErrorCode::ServiceUnavailable => return Err(error),
            Err(_) => return Ok(Err(Failure::Failed)),
        };
        tx.commit().await?;
        Ok(Ok(self.done(Some(started.cookie))))
    }

    fn done(&self, session_cookie: Option<HeaderValue>) -> Done {
        Done {
            location: self.flow.return_to.clone(),
            session_cookie,
        }
    }
}

/// Creates the account for an unknown identity, or the failure that prevents it.
async fn new_account(
    conn: &mut PgConnection,
    identity: &VerifiedIdentity,
) -> Result<Result<Uuid, Failure>, ApiError> {
    if !identity.email_verified {
        return Ok(Err(Failure::EmailUnverified));
    }
    let Some(email) = identity.email.as_deref().and_then(valid_email) else {
        return Ok(Err(Failure::Failed));
    };
    let display_name = identity
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| (1..=80).contains(&name.chars().count()));
    Ok(store::create_account(
        conn,
        email,
        display_name,
        &identity.issuer,
        &identity.subject,
    )
    .await?
    .ok_or(Failure::LinkRequired))
}

/// Redirects a failed callback. Sign-in (and unknown flows) go to the sign-in page; link and
/// reauth go back to their `returnTo`. Only the fixed failure code is logged.
fn failed(
    mode: CookieMode,
    flow: Option<&StoredFlow>,
    failure: Failure,
) -> Result<Response, ApiError> {
    tracing::info!(failure = failure.code(), "google callback failed");
    let location = match flow {
        Some(flow) if flow.intent != Intent::Signin => {
            let separator = if flow.return_to.contains('?') {
                '&'
            } else {
                '?'
            };
            format!("{}{separator}error={}", flow.return_to, failure.code())
        }
        _ => format!("{SIGNIN_PAGE}?error={}", failure.code()),
    };
    redirect(&location, [cookie::clear(Kind::GoogleFlow, mode)])
}

/// 303 to `location` with each Set-Cookie value.
fn redirect(
    location: &str,
    cookies: impl IntoIterator<Item = HeaderValue>,
) -> Result<Response, ApiError> {
    let location =
        HeaderValue::from_str(location).map_err(|_| ApiError::unavailable("redirect_location"))?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(header::LOCATION, location);
    for cookie in cookies {
        headers.append(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// C03 configurable default for public endpoints without an identifier: ordinary write budget
/// per client IP.
fn limit(security: &Security, endpoint: &str, ip: Option<IpAddr>) -> Result<(), ApiError> {
    let ip = ip.map_or_else(|| "-".to_owned(), |ip| ip.to_string());
    security
        .limiter
        .check(&format!("{endpoint}:{ip}"), &WRITE_RULES)
}
