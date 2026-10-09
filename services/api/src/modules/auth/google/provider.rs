//! Google OpenID Connect through the maintained `openidconnect` crate: fixed-issuer discovery and
//! JWKS, the authorization URL, the PKCE code exchange and ID-token validation (signature,
//! algorithm, issuer, audience, expiry and nonce, plus `auth_time` for a fresh login). Provider
//! errors can carry response bodies,
//! codes and URLs, so they are reduced to [`ProviderError`] and never logged or returned.

use std::{
    env, fmt,
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use chrono::{TimeDelta, Utc};
use openidconnect::{
    AsyncHttpClient, AuthenticationFlow, AuthorizationCode, ClaimsVerificationError, ClientId,
    ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest,
    HttpResponse, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    RequestTokenError, Scope, TokenResponse,
    core::{CoreAuthPrompt, CoreClient, CoreProviderMetadata, CoreResponseType},
    http,
    url::Url,
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

use crate::modules::auth::session::secret::Secret;

/// The only issuer accepted in production (C04: fixed Google discovery).
pub const GOOGLE_ISSUER: &str = "https://accounts.google.com";

/// Discovery and JWKS are cached this long; Google rotates signing keys with overlap.
const METADATA_TTL: Duration = Duration::from_secs(60 * 60);
/// A token signed by an unknown key refetches the JWKS at most this often.
const MIN_REFRESH: Duration = Duration::from_secs(60);
const MAX_BODY: usize = 1024 * 1024;
/// A fresh login must have happened at Google at most this long before the callback.
const MAX_AUTH_AGE: TimeDelta = TimeDelta::minutes(5);
/// Tolerated clock difference for an `auth_time` slightly in the future.
const CLOCK_SKEW: TimeDelta = TimeDelta::minutes(1);

/// Google OAuth client credentials. `Debug` omits the secret.
#[derive(Clone)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
    /// [`GOOGLE_ISSUER`] in production; tests point it at a local OIDC fixture.
    pub issuer: String,
}

impl GoogleConfig {
    /// Reads `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET`. Errors never echo values.
    pub fn from_env() -> Result<Self, &'static str> {
        let read = |name| env::var(name).ok().filter(|value| !value.trim().is_empty());
        Ok(Self {
            client_id: read("GOOGLE_CLIENT_ID").ok_or("GOOGLE_CLIENT_ID is required")?,
            client_secret: read("GOOGLE_CLIENT_SECRET")
                .ok_or("GOOGLE_CLIENT_SECRET is required")?,
            issuer: GOOGLE_ISSUER.to_owned(),
        })
    }
}

impl fmt::Debug for GoogleConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoogleConfig")
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

/// Why a provider step failed, without any provider text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderError {
    /// Discovery, JWKS or the token endpoint could not be reached or returned garbage.
    Unavailable,
    /// The code was refused, or the ID token failed validation.
    Rejected,
}

impl ProviderError {
    pub fn category(self) -> &'static str {
        match self {
            Self::Unavailable => "google_unavailable",
            Self::Rejected => "google_rejected",
        }
    }
}

/// The validated claims SceneCask uses. Google access tokens are dropped unread.
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
}

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

pub struct Google {
    http: Http,
    client_id: ClientId,
    client_secret: ClientSecret,
    issuer: IssuerUrl,
    redirect: RedirectUrl,
    metadata: Mutex<Option<(Instant, CoreProviderMetadata)>>,
}

impl Google {
    /// `public_origin` is the serialized origin from `SecurityConfig`; the callback is
    /// `{public_origin}/api/v1/auth/google/callback`.
    pub fn new(config: GoogleConfig, public_origin: &str) -> Result<Self, &'static str> {
        const INVALID: &str = "Google sign-in configuration is invalid";
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            // OIDC endpoints never redirect; following one could leak the client secret.
            .redirect(reqwest::redirect::Policy::none())
            // An authorization code is single use, so a token request is never replayed.
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| INVALID)?;
        Ok(Self {
            http: Http(client),
            client_id: ClientId::new(config.client_id),
            client_secret: ClientSecret::new(config.client_secret),
            issuer: IssuerUrl::new(config.issuer).map_err(|_| INVALID)?,
            redirect: RedirectUrl::new(format!("{public_origin}/api/v1/auth/google/callback"))
                .map_err(|_| INVALID)?,
            metadata: Mutex::new(None),
        })
    }

    /// The issuer stored with every Google identity.
    pub fn issuer(&self) -> &str {
        self.issuer.as_str()
    }

    /// The Google authorization URL for this flow's state, nonce and PKCE challenge, requesting
    /// only `openid email profile`. `fresh_login` asks Google to authenticate the user again
    /// (`max_age=0`, `prompt=login`) instead of reusing its existing browser session.
    pub(super) async fn authorize_url(
        &self,
        state: String,
        nonce: String,
        challenge: PkceCodeChallenge,
        fresh_login: bool,
    ) -> Result<Url, ProviderError> {
        let client = self.client(false).await?;
        let mut request = client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("profile".to_owned()))
            .set_pkce_challenge(challenge);
        if fresh_login {
            request = request
                .set_max_age(Duration::ZERO)
                .add_prompt(CoreAuthPrompt::Login);
        }
        let (url, _, _) = request.url();
        Ok(url)
    }

    /// Exchanges `code` with the flow's PKCE verifier and validates the ID token, including that
    /// its nonce is the flow's nonce secret, whose hash is `nonce_hash`. With `fresh_login` the
    /// token must also carry an `auth_time` within [`MAX_AUTH_AGE`]: `prompt=login` travels
    /// through the browser and could be stripped, so only the signed claim proves a new login.
    pub(super) async fn exchange(
        &self,
        code: String,
        verifier: String,
        nonce_hash: &[u8],
        fresh_login: bool,
    ) -> Result<VerifiedIdentity, ProviderError> {
        let client = self.client(false).await?;
        let response = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|_| ProviderError::Unavailable)?
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&self.http)
            .await
            .map_err(|error| match error {
                RequestTokenError::ServerResponse(_) => ProviderError::Rejected,
                _ => ProviderError::Unavailable,
            })?;
        let id_token = response.id_token().ok_or(ProviderError::Rejected)?;
        let verify = |client: &Client| {
            let nonce_matches = |nonce: Option<&Nonce>| match nonce
                .and_then(|nonce| Secret::decode(nonce.secret()))
            {
                Some(nonce) if bool::from(nonce.hash().ct_eq(nonce_hash)) => Ok(()),
                _ => Err("nonce mismatch".to_owned()),
            };
            let recent = move |auth_time: Option<chrono::DateTime<Utc>>| {
                let now = Utc::now();
                match auth_time {
                    _ if !fresh_login => Ok(()),
                    Some(at) if at <= now + CLOCK_SKEW && now - at <= MAX_AUTH_AGE => Ok(()),
                    _ => Err("stale authentication".to_owned()),
                }
            };
            id_token
                .claims(
                    &client.id_token_verifier().set_auth_time_verifier_fn(recent),
                    nonce_matches,
                )
                .map(|claims| VerifiedIdentity {
                    issuer: claims.issuer().as_str().to_owned(),
                    subject: claims.subject().as_str().to_owned(),
                    email: claims.email().map(|email| email.as_str().to_owned()),
                    email_verified: claims.email_verified() == Some(true),
                    name: claims
                        .name()
                        .and_then(|name| name.get(None))
                        .map(|name| name.as_str().to_owned()),
                })
        };
        let identity = match verify(&client) {
            Ok(identity) => identity,
            // A key rotated since the cached JWKS: refetch once, then verify again.
            Err(ClaimsVerificationError::SignatureVerification(_)) => {
                let client = self.client(true).await?;
                verify(&client).map_err(|_| ProviderError::Rejected)?
            }
            Err(_) => return Err(ProviderError::Rejected),
        };
        // 0001 requires a non-empty issuer and subject; Google subjects are at most 255 chars.
        if identity.subject.is_empty() || identity.subject.len() > 255 {
            return Err(ProviderError::Rejected);
        }
        Ok(identity)
    }

    /// A client from cached discovery metadata and JWKS. `refresh` refetches unless the cache is
    /// younger than [`MIN_REFRESH`].
    async fn client(&self, refresh: bool) -> Result<Client, ProviderError> {
        let mut cache = self.metadata.lock().await;
        let fresh = cache.as_ref().is_some_and(|(fetched, _)| {
            let age = fetched.elapsed();
            age < METADATA_TTL && !(refresh && age >= MIN_REFRESH)
        });
        if !fresh {
            let metadata = CoreProviderMetadata::discover_async(self.issuer.clone(), &self.http)
                .await
                .map_err(|_| ProviderError::Unavailable)?;
            if metadata.token_endpoint().is_none() {
                return Err(ProviderError::Unavailable);
            }
            *cache = Some((Instant::now(), metadata));
        }
        let (_, metadata) = cache.as_ref().ok_or(ProviderError::Unavailable)?;
        Ok(CoreClient::from_provider_metadata(
            metadata.clone(),
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(self.redirect.clone()))
    }
}

/// `openidconnect` transport over the API's reqwest version, with a response size cap.
struct Http(reqwest::Client);

/// A transport failure; the cause is discarded because it can contain URLs or bodies.
#[derive(Debug)]
pub struct HttpFailure;

impl fmt::Display for HttpFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider request failed")
    }
}

impl std::error::Error for HttpFailure {}

impl<'c> AsyncHttpClient<'c> for Http {
    type Error = HttpFailure;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, HttpFailure>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let url = reqwest::Url::parse(&parts.uri.to_string()).map_err(|_| HttpFailure)?;
            let mut response = self
                .0
                .request(parts.method, url)
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .map_err(|_| HttpFailure)?;
            if response
                .content_length()
                .is_some_and(|length| length > MAX_BODY as u64)
            {
                return Err(HttpFailure);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| HttpFailure)? {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(HttpFailure);
                }
                body.extend_from_slice(&chunk);
            }
            let mut converted = http::Response::new(body);
            *converted.status_mut() = response.status();
            *converted.headers_mut() = response.headers().clone();
            Ok(converted)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_client_secret() {
        let config = GoogleConfig {
            client_id: "client".into(),
            client_secret: "secret-value".into(),
            issuer: GOOGLE_ISSUER.into(),
        };
        assert!(!format!("{config:?}").contains("secret-value"));
    }

    #[test]
    fn callback_is_under_the_public_origin() {
        let config = GoogleConfig {
            client_id: "client".into(),
            client_secret: "secret".into(),
            issuer: GOOGLE_ISSUER.into(),
        };
        let google = Google::new(config.clone(), "https://scenecask.example").unwrap();
        assert_eq!(
            google.redirect.as_str(),
            "https://scenecask.example/api/v1/auth/google/callback"
        );
        assert_eq!(google.issuer(), GOOGLE_ISSUER);
        let invalid = GoogleConfig {
            issuer: "not a url".into(),
            ..config
        };
        assert!(Google::new(invalid, "https://scenecask.example").is_err());
    }
}
