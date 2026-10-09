//! Shared integration harness. Keep feature routes and assertions in their test modules.
// Each integration binary compiles this module independently and uses a different subset.
#![allow(dead_code)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};

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
        auth::{AuthMail, password::REAUTH_WINDOW, session::start_session},
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

/// An app serving `routes` behind the C03 layers, with a verified member signed in.
pub async fn member_app(
    pool: &PgPool,
    routes: impl FnOnce(&Security) -> Router,
) -> (TestApp, Router, TestUser) {
    let test = TestApp::new(pool, |_| {});
    let member = TestUser::create(&test.security, true, "member").await;
    let app = test.routes(routes(&test.security));
    (test, app, member)
}

/// A `member_app` with its pool, for feature tests that seed rows and add their own helpers in
/// an `impl MemberFixture` block.
pub struct MemberFixture {
    pub app: Router,
    pub pool: PgPool,
    pub test: TestApp,
    pub ana: TestUser,
}

pub async fn member_fixture(
    pool: PgPool,
    routes: impl FnOnce(&Security) -> Router,
) -> MemberFixture {
    let (test, app, ana) = member_app(&pool, routes).await;
    MemberFixture {
        app,
        pool,
        test,
        ana,
    }
}

/// Asserts that `user`'s keyed JSON write is rejected 403 CSRF_FAILED without its CSRF token.
pub async fn assert_csrf_required(
    app: &Router,
    user: &TestUser,
    method: Method,
    path: &str,
    body: &str,
) {
    let key = Uuid::new_v4().to_string();
    let mut call = Call::write(method, path, body)
        .idempotent(&key)
        .signed(&user.signed);
    call.csrf = None;
    let (status, headers, body) = send(app, call).await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
}

pub async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Waits until `task` finishes or another backend is blocked on a row or table lock, so a test
/// holding a transaction open can commit once the request under test is waiting on it.
pub async fn until_blocked<T>(pool: &PgPool, task: &tokio::task::JoinHandle<T>) {
    for _ in 0..500 {
        if task.is_finished()
            || count(
                pool,
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .await
                > 0
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("request neither finished nor blocked on a lock");
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

/// Makes every session of `user_id` older than the reauthentication window, with a minute's margin.
pub async fn stale_reauth(pool: &PgPool, user_id: Uuid) {
    sqlx::query(
        "UPDATE sessions SET reauthenticated_at = now() - make_interval(secs => $2) WHERE user_id = $1",
    )
    .bind(user_id)
    .bind((REAUTH_WINDOW.as_secs() + 60) as f64)
    .execute(pool)
    .await
    .unwrap();
}

/// Gives `user_id` a password sign-in method whose hash no password matches.
pub async fn add_password(pool: &PgPool, user_id: Uuid) {
    sqlx::query(
        "INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, '$argon2id$x')",
    )
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

static PROVIDER_IDS: AtomicI64 = AtomicI64::new(1);

/// A complete show with regular and special episodes. `None` air dates are undated. Episode
/// titles and stills use the protected sentinels that `assert_error` and the tests look for.
pub async fn seed_show(
    pool: &PgPool,
    title: &str,
    status: &str,
    episodes: &[(i32, i32, Option<&str>)],
) -> (Uuid, Vec<Uuid>) {
    let next = || PROVIDER_IDS.fetch_add(1, Ordering::SeqCst);
    let show: Uuid = sqlx::query_scalar(
        "INSERT INTO shows (tmdb_id, title, status, poster_path, fetched_at, complete_import)
         VALUES ($1, $2, $3, '/poster.jpg', now(), true) RETURNING id",
    )
    .bind(next())
    .bind(title)
    .bind(status)
    .fetch_one(pool)
    .await
    .unwrap();
    let mut ids = Vec::new();
    for &(season, number, air_date) in episodes {
        let season_id: Uuid = sqlx::query_scalar(
            "WITH existing AS (SELECT id FROM seasons WHERE show_id = $2 AND number = $3),
                  created AS (INSERT INTO seasons (tmdb_id, show_id, number)
                              SELECT $1, $2, $3 WHERE NOT EXISTS (SELECT 1 FROM existing)
                              RETURNING id)
             SELECT id FROM existing UNION ALL SELECT id FROM created",
        )
        .bind(next())
        .bind(show)
        .bind(season)
        .fetch_one(pool)
        .await
        .unwrap();
        ids.push(
            sqlx::query_scalar(
                "INSERT INTO episodes (tmdb_id, show_id, season_id, number, title, overview, still_path, air_date)
                 VALUES ($1, $2, $3, $4, 'Protected title', 'Protected overview',
                         '/protected-episode.jpg', $5::date) RETURNING id",
            )
            .bind(next())
            .bind(show)
            .bind(season_id)
            .bind(number)
            .bind(air_date)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    (show, ids)
}

/// A regular episode released long ago, for `seed_show`.
pub fn released(season: i32, number: i32) -> (i32, i32, Option<&'static str>) {
    (season, number, Some("2024-01-01"))
}
