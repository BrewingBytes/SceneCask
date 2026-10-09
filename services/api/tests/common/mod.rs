//! Shared integration harness. Keep feature routes and assertions in their test modules.
// Each integration binary compiles this module independently and uses a different subset.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::Response,
};
use lettre::Message;
use scenecask_api::{
    mail::MailTransport,
    middleware::{self, Security, SecurityConfig},
    modules::{
        auth::{AuthMail, session::start_session},
        mail::Worker,
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub const ORIGIN: &str = "https://scenecask.example";
pub const COOKIE: &str = "__Host-scenecask";

pub struct TestApp {
    pub security: Security,
}
impl TestApp {
    pub fn new(pool: &PgPool, configure: impl FnOnce(&mut SecurityConfig)) -> Self {
        let mut config = SecurityConfig::new(ORIGIN).unwrap();
        configure(&mut config);
        Self {
            security: Security::new(pool.clone(), config),
        }
    }
    pub fn routes(&self, routes: Router) -> Router {
        middleware::apply(routes, &self.security)
    }
}

pub async fn user(pool: &PgPool, email: &str, verified: bool, role: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (normalized_email, display_name, verified_at, role)
         VALUES ($1, 'Ana', CASE WHEN $2 THEN now() END, $3) RETURNING id",
    )
    .bind(email)
    .bind(verified)
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub struct Signed {
    pub cookie: String,
    pub csrf: String,
    pub hash: Vec<u8>,
}

pub async fn sign_in(security: &Security, user_id: Uuid) -> Signed {
    let mut conn = security.pool.acquire().await.unwrap();
    let started = start_session(&mut conn, &security.config, None, user_id)
        .await
        .unwrap();
    signed(started.cookie.to_str().unwrap(), started.session.csrf_token)
}

pub fn signed(set_cookie: &str, csrf: String) -> Signed {
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    let secret = cookie.strip_prefix(&format!("{COOKIE}=")).unwrap();
    let hash = sha256(secret);
    Signed { cookie, csrf, hash }
}

/// Mirrors the store's hash: SHA-256 of the decoded 32-byte cookie secret.
pub fn sha256(secret: &str) -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    Sha256::digest(URL_SAFE_NO_PAD.decode(secret).unwrap()).to_vec()
}

pub async fn count_session(pool: &PgPool, table: &str, hash: &[u8]) -> i64 {
    let column = if table == "sessions" {
        "id_hash"
    } else {
        "session_id"
    };
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM {table} WHERE {column} = $1"
    )))
    .bind(hash)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub struct Call<'a> {
    pub method: Method,
    pub path: &'a str,
    pub cookie: Option<&'a str>,
    pub csrf: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub idempotency_key: Option<&'a str>,
    pub body: &'a str,
}

impl<'a> Call<'a> {
    pub fn get(path: &'a str) -> Self {
        Self {
            method: Method::GET,
            path,
            cookie: None,
            csrf: None,
            origin: None,
            content_type: None,
            idempotency_key: None,
            body: "",
        }
    }

    pub fn post(path: &'a str, body: &'a str) -> Self {
        Self {
            method: Method::POST,
            origin: Some(ORIGIN),
            content_type: Some("application/json"),
            body,
            ..Self::get(path)
        }
    }

    /// A JSON mutation with `method` (PUT, PATCH, DELETE), shaped like `post`.
    pub fn write(method: Method, path: &'a str, body: &'a str) -> Self {
        Self {
            method,
            ..Self::post(path, body)
        }
    }

    pub fn idempotent(self, key: &'a str) -> Self {
        Self {
            idempotency_key: Some(key),
            ..self
        }
    }

    pub fn signed(self, signed: &'a Signed) -> Self {
        Self {
            cookie: Some(&signed.cookie),
            csrf: Some(&signed.csrf),
            ..self
        }
    }
}

pub async fn send(app: &Router, call: Call<'_>) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder().method(call.method).uri(call.path);
    for (name, value) in [
        (header::COOKIE.as_str(), call.cookie),
        ("x-csrf-token", call.csrf),
        (header::ORIGIN.as_str(), call.origin),
        (header::CONTENT_TYPE.as_str(), call.content_type),
        ("idempotency-key", call.idempotency_key),
    ] {
        if let Some(value) = value {
            request = request.header(name, value);
        }
    }
    let request = request
        .header(header::CONTENT_LENGTH, call.body.len())
        .body(Body::from(call.body.to_owned()))
        .unwrap();
    parts(app.clone().oneshot(request).await.unwrap()).await
}

pub async fn parts(response: Response) -> (StatusCode, HeaderMap, Value) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, headers, body)
}

pub fn set_cookies(headers: &HeaderMap) -> Vec<&str> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect()
}

/// Asserts the C03 envelope, that the request ID matches the header, and that the body holds no
/// user data.
pub fn assert_error(
    status: StatusCode,
    headers: &HeaderMap,
    body: &Value,
    expected: u16,
    code: &str,
) {
    assert_eq!(status.as_u16(), expected, "{body}");
    assert_eq!(body["error"]["code"], code);
    assert_eq!(
        body["error"]["requestId"].as_str().unwrap(),
        headers["x-request-id"].to_str().unwrap()
    );
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    for sentinel in [
        "example.test",
        "Protected",
        "/protected-episode.jpg",
        "secret-value",
    ] {
        assert!(
            !body.to_string().contains(sentinel),
            "error leaked protected fixture data"
        );
    }
    assert!(body.get("user").is_none());
}

/// A fresh account/session; use `user` + `sign_in` separately for session lifecycle tests.
pub struct TestUser {
    pub id: Uuid,
    pub signed: Signed,
}
impl TestUser {
    pub async fn create(security: &Security, verified: bool, role: &str) -> Self {
        let id = user(
            &security.pool,
            &format!("{}@example.test", Uuid::new_v4()),
            true,
            role,
        )
        .await;
        let signed = sign_in(security, id).await;
        // Exercise guards with a session that predates verification being revoked.
        if !verified {
            sqlx::query("UPDATE users SET verified_at = NULL WHERE id = $1")
                .bind(id)
                .execute(&security.pool)
                .await
                .unwrap();
        }
        Self { id, signed }
    }
}

pub async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Runs `sql` and asserts PostgreSQL rejects it with `code` (SQLSTATE).
pub async fn assert_sqlstate(pool: &PgPool, code: &str, sql: &str) {
    assert_sqlstate_any(pool, &[code], sql).await;
}

/// Like `assert_sqlstate`, for errors whose SQLSTATE differs across PostgreSQL versions.
pub async fn assert_sqlstate_any(pool: &PgPool, codes: &[&str], sql: &str) {
    let error = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .expect_err(&format!("expected SQLSTATE {codes:?} for: {sql}"));
    let actual = error.as_database_error().and_then(|e| e.code());
    assert!(
        actual.as_deref().is_some_and(|code| codes.contains(&code)),
        "expected SQLSTATE {codes:?}, got {actual:?} for: {sql}"
    );
}

/// Convenience for JSON feature endpoints; `Call` supports malformed/raw security probes.
pub async fn call(
    app: &Router,
    path: &str,
    user: Option<&TestUser>,
    body: Option<&str>,
    csrf: bool,
) -> (StatusCode, Value) {
    let mut request = match body {
        Some(body) => Call::post(path, body),
        None => Call::get(path),
    };
    if let Some(user) = user {
        request = request.signed(&user.signed);
    }
    if !csrf {
        request.csrf = None;
    }
    let (status, headers, body) = send(app, request).await;
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    // JSON endpoints must never silently fall back to text.
    assert!(body.is_object());
    (status, body)
}

/// `{message:"Check your email."}`, the enumeration-safe body of email-sending auth requests.
pub fn check_email() -> Value {
    json!({ "message": "Check your email." })
}

/// Asserts the generic, enumeration-safe 202.
pub fn assert_generic(response: (StatusCode, Value)) {
    assert_eq!(response, (StatusCode::ACCEPTED, check_email()));
}

/// Records each sent message as its formatted RFC 5322 text.
#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    pub fn sent(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl MailTransport for Recorder {
    async fn send(&self, message: Message) -> Result<(), &'static str> {
        let formatted = String::from_utf8(message.formatted()).unwrap();
        self.0.lock().unwrap().push(formatted);
        Ok(())
    }
}

/// An SMTP outage.
pub struct Down;

impl MailTransport for Down {
    async fn send(&self, _: Message) -> Result<(), &'static str> {
        Err("email delivery unavailable")
    }
}

/// An outbox worker composing account emails over `transport`.
pub fn mail_worker<T: MailTransport + Sync>(pool: &PgPool, transport: T) -> Worker<T, AuthMail> {
    let composer = AuthMail::new(
        pool.clone(),
        ORIGIN.to_owned(),
        "SceneCask <no-reply@scenecask.test>".parse().unwrap(),
    );
    Worker::new(pool.clone(), transport, composer)
}

/// Sends every due message through a fresh recorder and returns the recorded messages.
pub async fn deliver(pool: &PgPool) -> Vec<String> {
    let recorder = Recorder::default();
    mail_worker(pool, recorder.clone())
        .run_once()
        .await
        .unwrap();
    recorder.sent()
}

/// Undoes the quoted-printable soft line breaks and `=3D` escapes lettre uses for long lines.
pub fn decoded(message: &str) -> String {
    message.replace("=\r\n", "").replace("=3D", "=")
}

/// The token from an emailed link to `path` (`…<path>#token=<43 base64url chars>`).
pub fn link_token(message: &str, path: &str) -> String {
    let message = decoded(message);
    let marker = format!("{ORIGIN}{path}#token=");
    let start = message.find(&marker).expect("emailed link") + marker.len();
    let token: String = message[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert_eq!(token.len(), 43, "token must be a full 256-bit secret");
    token
}

pub async fn user_id(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

pub async fn password_hash(pool: &PgPool, user_id: Uuid) -> String {
    sqlx::query_scalar("SELECT argon2_hash FROM password_credentials WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every stored auth/outbox value as text, to prove no plaintext secret was persisted.
pub async fn stored_text(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT concat_ws(' ',
             (SELECT string_agg(row_to_json(u)::text, ' ') FROM users u),
             (SELECT string_agg(row_to_json(p)::text, ' ') FROM password_credentials p),
             (SELECT string_agg(row_to_json(t)::text, ' ') FROM auth_tokens t),
             (SELECT string_agg(row_to_json(o)::text, ' ') FROM outbox o))",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Moves this user's emailed tokens `seconds` into the past.
pub async fn age_tokens(pool: &PgPool, user_id: Uuid, seconds: i64) {
    sqlx::query(
        "UPDATE auth_tokens SET created_at = created_at - make_interval(secs => $2),
                                expires_at = expires_at - make_interval(secs => $2)
         WHERE user_id = $1",
    )
    .bind(user_id)
    .bind(seconds as f64)
    .execute(pool)
    .await
    .unwrap();
}

/// Makes every undelivered outbox message due now, skipping retry backoff.
pub async fn make_due(pool: &PgPool) {
    sqlx::query("UPDATE outbox SET available_at = now() WHERE delivered_at IS NULL")
        .execute(pool)
        .await
        .unwrap();
}

/// A one-hour episode reveal grant on session `hash`.
pub async fn grant(pool: &PgPool, hash: &[u8], user_id: Uuid) {
    sqlx::query(
        "INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
         VALUES ($1, $2, 'episode_details', gen_random_uuid(), now() + interval '1 hour')",
    )
    .bind(hash)
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Disables an account, as an operator or account deletion would.
pub async fn disable(pool: &PgPool, user_id: Uuid) {
    sqlx::query("UPDATE users SET disabled_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
}
