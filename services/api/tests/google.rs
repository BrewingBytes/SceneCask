//! R09 Google sign-in, explicit linking, reauth and unlinking (C04) against real PostgreSQL and a
//! local OIDC provider (tests/google_fixtures/README.md). The provider enforces client
//! authentication, the redirect URI, single-use codes and PKCE; each test signs the ID token
//! claims it needs, so wrong issuer, audience, nonce, expiry and signature are exercised through
//! the real `openidconnect` validation.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Form, Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode, header},
    routing::{get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use openidconnect::{
    JsonWebKeyId, PrivateSigningKey,
    core::{CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey},
    url::Url,
};
use scenecask_api::{
    middleware::Security,
    modules::auth::{
        google::{self, Google, GoogleConfig},
        identities, session,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tracing_test::traced_test;
use uuid::Uuid;

mod common;
use common::{
    COOKIE, Call, ORIGIN, Signed, TestApp, add_password, assert_error, count, send, set_cookies,
    sha256, sign_in, signed, stale_reauth, user,
};

const CLIENT_ID: &str = "scenecask-test-client";
const CLIENT_SECRET: &str = "secret-value-client";
const KID: &str = "fixture-key";
const FLOW_COOKIE: &str = "__Host-scenecask-google";
const CALLBACK: &str = "https://scenecask.example/api/v1/auth/google/callback";
const EMAIL: &str = "ana@example.test";

/// The local provider: discovery, JWKS and a token endpoint for codes granted by the test.
struct Provider {
    issuer: String,
    key: CoreRsaPrivateSigningKey,
    foreign: CoreRsaPrivateSigningKey,
    grants: Mutex<HashMap<String, Grant>>,
}

struct Grant {
    challenge: String,
    id_token: String,
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    code_verifier: String,
}

fn key(file: &str) -> CoreRsaPrivateSigningKey {
    let pem = std::fs::read_to_string(format!(
        "{}/tests/google_fixtures/{file}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    CoreRsaPrivateSigningKey::from_pem(&pem, Some(JsonWebKeyId::new(KID.into()))).unwrap()
}

impl Provider {
    async fn start() -> Arc<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = Arc::new(Self {
            issuer: format!("http://{}", listener.local_addr().unwrap()),
            key: key("signing-key.pem"),
            foreign: key("foreign-key.pem"),
            grants: Mutex::default(),
        });
        let routes = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(provider.clone());
        tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        provider
    }

    /// Registers `code` for the PKCE `challenge`, returning an ID token with `claims`.
    fn grant(&self, code: &str, challenge: &str, claims: &Value) {
        self.grant_signed(code, challenge, claims, &self.key);
    }

    fn grant_signed(
        &self,
        code: &str,
        challenge: &str,
        claims: &Value,
        key: &CoreRsaPrivateSigningKey,
    ) {
        let header = URL_SAFE_NO_PAD.encode(json!({"alg": "RS256", "kid": KID}).to_string());
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let input = format!("{header}.{payload}");
        let signature = key
            .sign(
                &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                input.as_bytes(),
            )
            .unwrap();
        self.grants.lock().unwrap().insert(
            code.to_owned(),
            Grant {
                challenge: challenge.to_owned(),
                id_token: format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature)),
            },
        );
    }
}

async fn discovery(State(provider): State<Arc<Provider>>) -> Json<Value> {
    let issuer = &provider.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks(State(provider): State<Arc<Provider>>) -> Json<Value> {
    Json(json!({ "keys": [provider.key.as_verification_key()] }))
}

/// Issues the granted ID token once, for the right client, redirect URI and PKCE verifier.
async fn token(
    State(provider): State<Arc<Provider>>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> (StatusCode, Json<Value>) {
    let basic = format!(
        "Basic {}",
        STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
    );
    let grant = provider.grants.lock().unwrap().remove(&form.code);
    let valid = grant.filter(|grant| {
        headers[header::AUTHORIZATION] == basic.as_str()
            && form.grant_type == "authorization_code"
            && form.redirect_uri == CALLBACK
            && URL_SAFE_NO_PAD.encode(Sha256::digest(form.code_verifier.as_bytes()))
                == grant.challenge
    });
    match valid {
        Some(grant) => (
            StatusCode::OK,
            Json(json!({
                "access_token": "google-access-token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "google-refresh-token",
                "id_token": grant.id_token,
            })),
        ),
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant"})),
        ),
    }
}

struct Harness {
    security: Security,
    app: Router,
    provider: Arc<Provider>,
    issuer: String,
}

async fn harness(pool: &PgPool) -> Harness {
    let provider = Provider::start().await;
    let test = TestApp::new(pool, |_| {});
    let config = GoogleConfig {
        client_id: CLIENT_ID.into(),
        client_secret: CLIENT_SECRET.into(),
        issuer: provider.issuer.clone(),
    };
    let google = Arc::new(Google::new(config, ORIGIN).unwrap());
    let routes = google::routes(test.security.clone(), google.clone())
        .merge(identities::routes(test.security.clone(), &google))
        .merge(session::routes(test.security.clone()));
    Harness {
        app: test.routes(routes),
        security: test.security,
        issuer: provider.issuer.clone(),
        provider,
    }
}

/// A started flow: the values sent to Google and the binding cookie.
struct Flow {
    state: String,
    nonce: String,
    challenge: String,
    binding: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl Harness {
    /// `GET /auth/google/start`, asserting the authorization request it redirects to.
    async fn start(&self, query: &str, signed: Option<&Signed>) -> Flow {
        let (status, headers, body) = self.start_raw(query, signed).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        let location = Url::parse(headers[header::LOCATION].to_str().unwrap()).unwrap();
        assert_eq!(
            location.as_str().split('?').next().unwrap(),
            format!("{}/authorize", self.issuer)
        );
        let params: HashMap<String, String> = location.query_pairs().into_owned().collect();
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["client_id"], CLIENT_ID);
        assert_eq!(params["redirect_uri"], CALLBACK);
        assert_eq!(params["scope"], "openid email profile");
        assert_eq!(params["code_challenge_method"], "S256");
        // Only reauth forces a new Google login instead of reusing Google's browser session.
        let reauth = query.contains("intent=reauth");
        assert_eq!(
            params.get("max_age").map(String::as_str),
            reauth.then_some("0")
        );
        assert_eq!(
            params.get("prompt").map(String::as_str),
            reauth.then_some("login")
        );
        assert!(!location.as_str().contains(CLIENT_SECRET));
        let binding = set_cookies(&headers)
            .into_iter()
            .find(|cookie| cookie.starts_with(&format!("{FLOW_COOKIE}=")))
            .expect("binding cookie");
        assert!(binding.ends_with("; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=600"));
        let binding = binding.split(';').next().unwrap().to_owned();
        assert_eq!(binding, format!("{FLOW_COOKIE}={}", params["state"]));
        Flow {
            state: params["state"].clone(),
            nonce: params["nonce"].clone(),
            challenge: params["code_challenge"].clone(),
            binding,
        }
    }

    async fn start_raw(
        &self,
        query: &str,
        signed: Option<&Signed>,
    ) -> (StatusCode, HeaderMap, Value) {
        let path = format!("/auth/google/start?{query}");
        let mut call = Call::get(&path);
        call.cookie = signed.map(|signed| signed.cookie.as_str());
        send(&self.app, call).await
    }

    /// Valid Google claims for `flow` and `subject`.
    fn claims(&self, flow: &Flow, subject: &str) -> Value {
        json!({
            "iss": self.issuer,
            "aud": CLIENT_ID,
            "sub": subject,
            "email": EMAIL,
            "email_verified": true,
            "name": "Ana Reyes",
            "nonce": flow.nonce,
            "auth_time": now(),
            "iat": now(),
            "exp": now() + 300,
        })
    }

    /// Grants `claims` for `flow` and follows the callback in the browser that started it.
    async fn finish(&self, flow: &Flow, claims: &Value, signed: Option<&Signed>) -> Landing {
        let code = Uuid::new_v4().to_string();
        self.provider.grant(&code, &flow.challenge, claims);
        self.callback(&format!("code={code}&state={}", flow.state), flow, signed)
            .await
    }

    async fn callback(&self, query: &str, flow: &Flow, signed: Option<&Signed>) -> Landing {
        let cookies = match signed {
            Some(signed) => format!("{}; {}", flow.binding, signed.cookie),
            None => flow.binding.clone(),
        };
        self.callback_with_cookies(query, Some(&cookies)).await
    }

    async fn callback_with_cookies(&self, query: &str, cookies: Option<&str>) -> Landing {
        let path = format!("/auth/google/callback?{query}");
        let mut call = Call::get(&path);
        call.cookie = cookies;
        let (status, headers, body) = send(&self.app, call).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(body, Value::Null);
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        let cookies = set_cookies(&headers);
        assert!(
            cookies.contains(
                &format!("{FLOW_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=0")
                    .as_str()
            ),
            "binding cookie must be cleared: {cookies:?}"
        );
        let session = cookies
            .iter()
            .find(|cookie| cookie.starts_with(&format!("{COOKIE}=")))
            .map(|cookie| cookie.split(';').next().unwrap().to_owned());
        Landing {
            location: headers[header::LOCATION].to_str().unwrap().to_owned(),
            session,
        }
    }

    /// Signs in a new Google-only account for `subject` and returns it with its session.
    async fn google_account(&self, subject: &str, email: &str) -> (Uuid, Signed) {
        let flow = self.start("intent=signin", None).await;
        let mut claims = self.claims(&flow, subject);
        claims["email"] = email.into();
        let landing = self.finish(&flow, &claims, None).await;
        assert_eq!(landing.location, "/onboarding/profile");
        let cookie = landing.session.expect("session cookie");
        let signed = self.session(&cookie).await;
        let id = self.owner(subject).await.unwrap();
        (id, signed)
    }

    /// The `Signed` handle (with CSRF token) for a session cookie.
    async fn session(&self, cookie: &str) -> Signed {
        let mut call = Call::get("/session");
        call.cookie = Some(cookie);
        let (_, _, body) = send(&self.app, call).await;
        signed(cookie, body["csrfToken"].as_str().unwrap().to_owned())
    }

    async fn owner(&self, subject: &str) -> Option<Uuid> {
        sqlx::query_scalar(
            "SELECT user_id FROM external_identities WHERE issuer = $1 AND subject = $2",
        )
        .bind(&self.issuer)
        .bind(subject)
        .fetch_optional(&self.security.pool)
        .await
        .unwrap()
    }

    async fn identities(&self, signed: &Signed) -> Value {
        let (status, _, body) = send(&self.app, Call::get("/me/identities").signed(signed)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn unlink(&self, signed: &Signed) -> (StatusCode, HeaderMap, Value) {
        send(
            &self.app,
            Call::write(Method::DELETE, "/me/identities/google", "").signed(signed),
        )
        .await
    }

    /// Accounts, identities, sessions and usable flows: what a failed callback must not change.
    async fn snapshot(&self) -> [i64; 4] {
        let pool = &self.security.pool;
        [
            count(pool, "SELECT count(*) FROM users").await,
            count(pool, "SELECT count(*) FROM external_identities").await,
            count(pool, "SELECT count(*) FROM sessions").await,
            count(
                pool,
                "SELECT count(*) FROM oauth_flows WHERE consumed_at IS NULL AND expires_at > now()",
            )
            .await,
        ]
    }
}

struct Landing {
    location: String,
    session: Option<String>,
}

#[sqlx::test]
#[traced_test]
async fn new_identity_creates_one_private_account_and_signs_in_again(pool: PgPool) {
    let h = harness(&pool).await;
    let flow = h.start("intent=signin&returnTo=/library", None).await;
    let stored: (Vec<u8>, Vec<u8>, Vec<u8>, String, String) = sqlx::query_as(
        "SELECT state_hash, nonce_hash, pkce_verifier_ciphertext, intent, return_to
         FROM oauth_flows WHERE expires_at BETWEEN now() + interval '599 seconds'
                                               AND now() + interval '600 seconds'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.0, sha256(&flow.state));
    assert_eq!(stored.1, sha256(&flow.nonce));
    assert_eq!(
        (stored.3.as_str(), stored.4.as_str()),
        ("signin", "/library")
    );
    let stored_text: String = sqlx::query_scalar("SELECT row_to_json(f)::text FROM oauth_flows f")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored_text.contains(&flow.state) && !stored_text.contains(&flow.nonce));

    let code = "first-code";
    h.provider
        .grant(code, &flow.challenge, &h.claims(&flow, "google-sub-1"));
    let landing = h
        .callback(&format!("code={code}&state={}", flow.state), &flow, None)
        .await;
    assert_eq!(landing.location, "/onboarding/profile");
    let signed = h.session(&landing.session.unwrap()).await;
    let (_, _, session) = send(&h.app, Call::get("/session").signed(&signed)).await;
    let user = &session["user"];
    assert_eq!(user["email"], EMAIL);
    assert_eq!(user["displayName"], "Ana Reyes");
    assert_eq!(user["handle"], Value::Null);
    assert_eq!(user["visibility"], "private");
    assert_eq!(user["verified"], true);
    assert_eq!(user["role"], "member");
    assert_eq!(
        h.identities(&signed).await,
        json!({"passwordEnabled": false, "googleLinked": true})
    );
    assert_eq!(h.snapshot().await, [1, 1, 1, 0]);
    // No Google token is retained anywhere.
    let everything: String = sqlx::query_scalar(
        "SELECT concat_ws(' ', (SELECT string_agg(row_to_json(u)::text, ' ') FROM users u),
                               (SELECT string_agg(row_to_json(i)::text, ' ') FROM external_identities i),
                               (SELECT string_agg(row_to_json(f)::text, ' ') FROM oauth_flows f))",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!everything.contains("google-access-token"));
    assert!(!everything.contains("google-refresh-token"));

    // The same subject signs into the same account and lands on returnTo, even with a new email.
    let flow = h.start("intent=signin&returnTo=/library", None).await;
    let mut claims = h.claims(&flow, "google-sub-1");
    claims["email"] = "renamed@example.test".into();
    let landing = h.finish(&flow, &claims, Some(&signed)).await;
    assert_eq!(landing.location, "/library");
    let again = h.session(&landing.session.unwrap()).await;
    assert_ne!(again.hash, signed.hash, "sign-in rotates the session");
    let (_, _, session) = send(&h.app, Call::get("/session").signed(&again)).await;
    assert_eq!(session["user"]["id"], user["id"]);
    assert_eq!(session["user"]["email"], EMAIL);
    assert_eq!(h.snapshot().await, [1, 1, 1, 0]);

    for secret in [
        code,
        flow.state.as_str(),
        flow.nonce.as_str(),
        EMAIL,
        CLIENT_SECRET,
        "google-access-token",
    ] {
        assert!(!logs_contain(secret), "logs must not contain secrets");
    }
}

#[sqlx::test]
async fn matching_email_requires_explicit_linking(pool: PgPool) {
    let h = harness(&pool).await;
    let existing = user(&pool, EMAIL, true, "member").await;
    add_password(&pool, existing).await;
    let before = h.snapshot().await;
    let flow = h.start("intent=signin", None).await;
    // Casefolded and padded: still the same email, still no merge.
    let mut claims = h.claims(&flow, "google-sub-2");
    claims["email"] = " ANA@Example.TEST ".into();
    let landing = h.finish(&flow, &claims, None).await;
    assert_eq!(landing.location, "/auth/signin?error=link_required");
    assert!(landing.session.is_none());
    assert_eq!(h.snapshot().await, [before[0], 0, 0, 0]);

    // Signed in with the existing method and freshly reauthenticated, the user links explicitly.
    let signed = sign_in(&h.security, existing).await;
    let flow = h
        .start("intent=link&returnTo=/settings", Some(&signed))
        .await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-2"), Some(&signed))
        .await;
    assert_eq!(landing.location, "/settings");
    assert!(landing.session.is_none());
    assert_eq!(h.owner("google-sub-2").await, Some(existing));
    assert_eq!(
        h.identities(&signed).await,
        json!({"passwordEnabled": true, "googleLinked": true})
    );

    // Google now signs into that account.
    let flow = h.start("intent=signin", None).await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-2"), None)
        .await;
    assert_eq!(landing.location, "/home");
    let google = h.session(&landing.session.unwrap()).await;
    let (_, _, session) = send(&h.app, Call::get("/session").signed(&google)).await;
    assert_eq!(session["user"]["id"], existing.to_string());
}

#[sqlx::test]
async fn invalid_tokens_and_flows_fail_without_mutation(pool: PgPool) {
    let h = harness(&pool).await;
    // Each case changes valid claims: a value replaces the claim, null removes it.
    let cases = [
        ("wrong nonce", json!({"nonce": "other-nonce"})),
        ("no nonce", json!({"nonce": null})),
        ("wrong audience", json!({"aud": "another-client"})),
        (
            "wrong issuer",
            json!({"iss": "https://accounts.google.com"}),
        ),
        ("expired", json!({"iat": now() - 7200, "exp": now() - 3600})),
        ("unverified email", json!({"email_verified": false})),
        ("missing email", json!({"email": null})),
        (
            "other audience too",
            json!({"aud": [CLIENT_ID, "another-client"]}),
        ),
        ("empty subject", json!({"sub": ""})),
    ];
    for (name, changes) in cases {
        let flow = h.start("intent=signin", None).await;
        let mut claims = h.claims(&flow, "google-sub-3");
        let claims_map = claims.as_object_mut().unwrap();
        for (claim, value) in changes.as_object().unwrap() {
            match value {
                Value::Null => claims_map.remove(claim),
                value => claims_map.insert(claim.clone(), value.clone()),
            };
        }
        let landing = h.finish(&flow, &claims, None).await;
        let expected = if name == "unverified email" {
            "/auth/signin?error=email_unverified"
        } else {
            "/auth/signin?error=failed"
        };
        assert_eq!(landing.location, expected, "{name}");
        assert!(landing.session.is_none(), "{name}");
        assert_eq!(h.snapshot().await, [0, 0, 0, 0], "{name}");
    }

    // A token signed by another key under the fixture's key ID.
    let flow = h.start("intent=signin", None).await;
    h.provider.grant_signed(
        "forged",
        &flow.challenge,
        &h.claims(&flow, "google-sub-3"),
        &h.provider.foreign,
    );
    let landing = h
        .callback(&format!("code=forged&state={}", flow.state), &flow, None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=failed");

    // A code granted for another PKCE challenge is refused by the provider.
    let flow = h.start("intent=signin", None).await;
    let other = h.start("intent=signin", None).await;
    h.provider
        .grant("pkce", &other.challenge, &h.claims(&flow, "google-sub-3"));
    let landing = h
        .callback(&format!("code=pkce&state={}", flow.state), &flow, None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=failed");

    // Unknown, malformed and cookie-less states.
    let flow = h.start("intent=signin", None).await;
    h.provider
        .grant("unbound", &flow.challenge, &h.claims(&flow, "google-sub-3"));
    let landing = h
        .callback_with_cookies(&format!("code=unbound&state={}", flow.state), None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=failed");
    for state in ["not-a-state", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"] {
        let landing = h
            .callback_with_cookies(&format!("code=x&state={state}"), Some(&flow.binding))
            .await;
        assert_eq!(landing.location, "/auth/signin?error=expired");
    }
    let (status, headers, body) = send(&h.app, Call::get("/auth/google/callback?code=x")).await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert_eq!(h.snapshot().await, [0, 0, 0, 1]);
}

#[sqlx::test]
async fn flows_are_single_use_expire_and_cancel_without_session(pool: PgPool) {
    let h = harness(&pool).await;
    // Replay: the same code and state a second time.
    let flow = h.start("intent=signin", None).await;
    h.provider
        .grant("once", &flow.challenge, &h.claims(&flow, "google-sub-4"));
    let query = format!("code=once&state={}", flow.state);
    let first = h.callback(&query, &flow, None).await;
    assert_eq!(first.location, "/onboarding/profile");
    let before = h.snapshot().await;
    h.provider
        .grant("once", &flow.challenge, &h.claims(&flow, "google-sub-4"));
    let replay = h.callback(&query, &flow, None).await;
    assert_eq!(replay.location, "/auth/signin?error=expired");
    assert!(replay.session.is_none());
    assert_eq!(h.snapshot().await, before);

    // Expired after ten minutes.
    let flow = h.start("intent=signin", None).await;
    sqlx::query(
        "UPDATE oauth_flows SET created_at = created_at - interval '11 minutes',
                                expires_at = expires_at - interval '11 minutes'
         WHERE consumed_at IS NULL",
    )
    .execute(&pool)
    .await
    .unwrap();
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-5"), None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=expired");
    assert_eq!(h.owner("google-sub-5").await, None);

    // Canceled at Google: recoverable, no session, and the flow is spent.
    let flow = h.start("intent=signin", None).await;
    let landing = h
        .callback(
            &format!("error=access_denied&state={}", flow.state),
            &flow,
            None,
        )
        .await;
    assert_eq!(landing.location, "/auth/signin?error=canceled");
    assert!(landing.session.is_none());
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-6"), None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=expired");
    assert_eq!(h.owner("google-sub-6").await, None);
    assert_eq!(h.snapshot().await, before);

    // Other Google errors are not a cancellation.
    for (error, code) in [
        ("temporarily_unavailable", "unavailable"),
        ("server_error", "unavailable"),
        ("invalid_request", "failed"),
        ("unauthorized_client", "failed"),
    ] {
        let flow = h.start("intent=signin", None).await;
        let landing = h
            .callback(&format!("error={error}&state={}", flow.state), &flow, None)
            .await;
        assert_eq!(landing.location, format!("/auth/signin?error={code}"));
        assert!(landing.session.is_none());
    }
    assert_eq!(h.snapshot().await, before);
}

#[sqlx::test]
async fn start_validates_intent_return_to_and_session(pool: PgPool) {
    let h = harness(&pool).await;
    for query in [
        "",
        "intent=merge",
        "intent=signin&returnTo=//evil.example/home",
        "intent=signin&returnTo=https://evil.example/home",
        "intent=signin&returnTo=/%5Cevil.example",
        "intent=signin&returnTo=/home%250d",
        "intent=signin&returnTo=/home%0d%0aLocation:%20x",
        "intent=signin&returnTo=/auth/verify",
    ] {
        let (status, headers, body) = h.start_raw(query, None).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    }
    for intent in ["link", "reauth"] {
        let (status, headers, body) = h.start_raw(&format!("intent={intent}"), None).await;
        assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    }
    assert_eq!(h.snapshot().await, [0, 0, 0, 0]);

    let (google_user, google_session) = h.google_account("google-sub-7", EMAIL).await;
    // Already linked: link is refused; a stale session must reauthenticate first.
    let (status, headers, body) = h.start_raw("intent=link", Some(&google_session)).await;
    assert_error(status, &headers, &body, 409, "IDENTITY_IN_USE");
    let password_user = user(&pool, "bo@example.test", true, "member").await;
    let password_session = sign_in(&h.security, password_user).await;
    let (status, headers, body) = h.start_raw("intent=reauth", Some(&password_session)).await;
    assert_error(status, &headers, &body, 409, "IDENTITY_LINK_REQUIRED");
    stale_reauth(&pool, password_user).await;
    let (status, headers, body) = h.start_raw("intent=link", Some(&password_session)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    stale_reauth(&pool, google_user).await;
    h.start(
        "intent=reauth&returnTo=/settings/data",
        Some(&google_session),
    )
    .await;
    let stored: (String, Option<Vec<u8>>) =
        sqlx::query_as("SELECT intent, session_id FROM oauth_flows WHERE consumed_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("reauth".into(), Some(google_session.hash.clone())));
}

#[sqlx::test]
async fn link_is_bound_to_the_starting_session_and_identity_owner(pool: PgPool) {
    let h = harness(&pool).await;
    let (owner, _) = h.google_account("google-sub-8", EMAIL).await;
    let other = user(&pool, "bo@example.test", true, "member").await;
    let signed = sign_in(&h.security, other).await;

    // The identity belongs to another account.
    let flow = h
        .start("intent=link&returnTo=/settings", Some(&signed))
        .await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-8"), Some(&signed))
        .await;
    assert_eq!(landing.location, "/settings?error=identity_in_use");
    assert_eq!(h.owner("google-sub-8").await, Some(owner));

    // Finished without the starting session, or in another one: no link.
    let flow = h
        .start("intent=link&returnTo=/settings?tab=methods", Some(&signed))
        .await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-9"), None)
        .await;
    assert_eq!(landing.location, "/settings?tab=methods&error=failed");
    let flow = h.start("intent=link", Some(&signed)).await;
    let second = sign_in(&h.security, other).await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-9"), Some(&second))
        .await;
    assert_eq!(landing.location, "/home?error=failed");
    assert_eq!(h.owner("google-sub-9").await, None);

    // Signing out deletes the session's pending flows.
    let flow = h.start("intent=link", Some(&signed)).await;
    let (status, _, _) = send(&h.app, Call::post("/auth/logout", "{}").signed(&signed)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-9"), Some(&signed))
        .await;
    assert_eq!(landing.location, "/auth/signin?error=expired");
    assert_eq!(h.owner("google-sub-9").await, None);
}

#[sqlx::test]
async fn concurrent_links_and_signins_resolve_to_one_owner(pool: PgPool) {
    let h = harness(&pool).await;
    let first = user(&pool, "first@example.test", true, "member").await;
    let second = user(&pool, "second@example.test", true, "member").await;
    let first_session = sign_in(&h.security, first).await;
    let second_session = sign_in(&h.security, second).await;
    let first_flow = h.start("intent=link", Some(&first_session)).await;
    let second_flow = h.start("intent=link", Some(&second_session)).await;
    let first_claims = h.claims(&first_flow, "google-sub-10");
    let second_claims = h.claims(&second_flow, "google-sub-10");
    let (a, b) = tokio::join!(
        h.finish(&first_flow, &first_claims, Some(&first_session)),
        h.finish(&second_flow, &second_claims, Some(&second_session)),
    );
    let mut locations = [a.location, b.location];
    locations.sort();
    assert_eq!(locations, ["/home", "/home?error=identity_in_use"]);
    let owner = h.owner("google-sub-10").await.unwrap();
    assert!(owner == first || owner == second);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM external_identities").await,
        1
    );

    // Two sign-ins of one new subject create one account.
    let one = h.start("intent=signin", None).await;
    let two = h.start("intent=signin", None).await;
    let (one_claims, two_claims) = (
        h.claims(&one, "google-sub-11"),
        h.claims(&two, "google-sub-11"),
    );
    let (a, b) = tokio::join!(
        h.finish(&one, &one_claims, None),
        h.finish(&two, &two_claims, None),
    );
    let mut locations = [a.location, b.location];
    locations.sort();
    assert_eq!(locations, ["/home", "/onboarding/profile"]);
    assert!(a.session.is_some() && b.session.is_some());
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 3);
}

#[sqlx::test]
async fn google_reauth_rotates_only_a_matching_session(pool: PgPool) {
    let h = harness(&pool).await;
    let (user_id, signed) = h.google_account("google-sub-12", EMAIL).await;
    stale_reauth(&pool, user_id).await;
    let (status, headers, body) = h.unlink(&signed).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");

    // Another Google account cannot reauthenticate this one.
    let flow = h
        .start("intent=reauth&returnTo=/settings", Some(&signed))
        .await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-13"), Some(&signed))
        .await;
    assert_eq!(landing.location, "/settings?error=reauth_mismatch");
    assert!(landing.session.is_none());
    let fresh: bool = sqlx::query_scalar(
        "SELECT reauthenticated_at > now() - interval '5 minutes' FROM sessions WHERE id_hash = $1",
    )
    .bind(&signed.hash)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!fresh);

    // The right Google account, but from an existing Google session rather than a new login.
    for auth_time in [
        Value::Null,
        (now() - 6 * 60).into(),
        (now() + 5 * 60).into(),
    ] {
        let flow = h
            .start("intent=reauth&returnTo=/settings", Some(&signed))
            .await;
        let mut claims = h.claims(&flow, "google-sub-12");
        match auth_time {
            Value::Null => drop(claims.as_object_mut().unwrap().remove("auth_time")),
            auth_time => claims["auth_time"] = auth_time,
        }
        let landing = h.finish(&flow, &claims, Some(&signed)).await;
        assert_eq!(landing.location, "/settings?error=failed");
        assert!(landing.session.is_none());
    }
    assert_eq!(
        common::count_session(&pool, "sessions", &signed.hash).await,
        1
    );

    let flow = h
        .start("intent=reauth&returnTo=/settings", Some(&signed))
        .await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-12"), Some(&signed))
        .await;
    assert_eq!(landing.location, "/settings");
    let rotated = h.session(&landing.session.unwrap()).await;
    assert_ne!(rotated.hash, signed.hash);
    assert_eq!(
        common::count_session(&pool, "sessions", &signed.hash).await,
        0
    );
    // Fresh now, but Google is the only sign-in method.
    let (status, headers, body) = h.unlink(&rotated).await;
    assert_error(status, &headers, &body, 409, "LAST_SIGNIN_METHOD");
    assert_eq!(h.owner("google-sub-12").await, Some(user_id));
}

#[sqlx::test]
async fn unlink_keeps_another_method_and_is_idempotent(pool: PgPool) {
    let h = harness(&pool).await;
    let (user_id, signed) = h.google_account("google-sub-14", EMAIL).await;
    add_password(&pool, user_id).await;
    let mut call = Call::write(Method::DELETE, "/me/identities/google", "").signed(&signed);
    call.csrf = None;
    let (status, headers, body) = send(&h.app, call).await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    let (status, _, body) = h.unlink(&signed).await;
    assert_eq!((status, body), (StatusCode::NO_CONTENT, Value::Null));
    assert_eq!(h.owner("google-sub-14").await, None);
    assert_eq!(
        h.identities(&signed).await,
        json!({"passwordEnabled": true, "googleLinked": false})
    );
    // The account and session remain; repeating is a no-op.
    assert_eq!(h.snapshot().await, [1, 0, 1, 0]);
    let (status, _, _) = h.unlink(&signed).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, headers, body) = send(&h.app, Call::get("/me/identities")).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");

    // Google sign-in for that subject is now a new identity whose email is taken.
    let flow = h.start("intent=signin", None).await;
    let landing = h
        .finish(&flow, &h.claims(&flow, "google-sub-14"), None)
        .await;
    assert_eq!(landing.location, "/auth/signin?error=link_required");
}
