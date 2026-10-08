//! Shared integration harness. Keep feature routes and assertions in their test modules.
// Each integration binary compiles this module independently and uses a different subset.
#![allow(dead_code)]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::Response,
};
use scenecask_api::{
    middleware::{self, Security, SecurityConfig},
    modules::auth::session::start_session,
};
use serde_json::Value;
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
