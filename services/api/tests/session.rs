//! C03/C04 session, CSRF, guard, limit and envelope tests against real PostgreSQL. Each
//! `#[sqlx::test]` gets a fresh migrated database. Expiry is driven by moving rows back in time
//! against the database clock that the session store evaluates.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use scenecask_api::{
    error::{ApiError, ErrorCode},
    middleware::{self, Security, SecurityConfig, rate_limit::Rule},
    modules::auth::session::{
        self, AuthUser, OperatorUser, VerifiedUser, reauthenticate, start_session, store,
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use tracing_test::traced_test;
use uuid::Uuid;

const ORIGIN: &str = "https://scenecask.example";
const COOKIE: &str = "__Host-scenecask";

fn security(pool: &PgPool) -> Security {
    let mut config = SecurityConfig::new(ORIGIN).unwrap();
    config.write_limit = Rule::per_minute(3);
    config.body_limit_bytes = 1024;
    Security::new(pool.clone(), config)
}

/// Session routes plus probe routes standing in for feature endpoints behind each guard.
fn app(security: &Security) -> Router {
    let pool = security.pool.clone();
    let probes = Router::new()
        .route(
            "/probe/write",
            post(|AuthUser(current): AuthUser| async move {
                // Appends one marker per executed write, so tests can count writes.
                sqlx::query("UPDATE users SET display_name = display_name || 'w' WHERE id = $1")
                    .bind(current.user.id)
                    .execute(&pool)
                    .await?;
                Ok::<_, ApiError>(StatusCode::NO_CONTENT)
            }),
        )
        .route("/probe/auth", get(|_: AuthUser| async { "ok" }))
        .route("/probe/verified", get(|_: VerifiedUser| async { "ok" }))
        .route("/probe/operator", get(|_: OperatorUser| async { "ok" }));
    middleware::apply(probes.merge(session::routes(security.clone())), security)
}

async fn user(pool: &PgPool, email: &str, verified: bool, role: &str) -> Uuid {
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

struct Signed {
    cookie: String,
    csrf: String,
    hash: Vec<u8>,
}

async fn sign_in(security: &Security, user_id: Uuid) -> Signed {
    let mut conn = security.pool.acquire().await.unwrap();
    let started = start_session(&mut conn, &security.config, None, user_id)
        .await
        .unwrap();
    signed(started.cookie.to_str().unwrap(), started.session.csrf_token)
}

fn signed(set_cookie: &str, csrf: String) -> Signed {
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    let secret = cookie.strip_prefix(&format!("{COOKIE}=")).unwrap();
    let hash = sha256(secret);
    Signed { cookie, csrf, hash }
}

/// Mirrors the store's hash: SHA-256 of the decoded 32-byte cookie secret.
fn sha256(secret: &str) -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    Sha256::digest(URL_SAFE_NO_PAD.decode(secret).unwrap()).to_vec()
}

async fn grant(pool: &PgPool, hash: &[u8], user_id: Uuid) {
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

async fn count(pool: &PgPool, table: &str, hash: &[u8]) -> i64 {
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

struct Call<'a> {
    method: Method,
    path: &'a str,
    cookie: Option<&'a str>,
    csrf: Option<&'a str>,
    origin: Option<&'a str>,
    content_type: Option<&'a str>,
    body: &'a str,
}

impl<'a> Call<'a> {
    fn get(path: &'a str) -> Self {
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

    fn post(path: &'a str, body: &'a str) -> Self {
        Self {
            method: Method::POST,
            origin: Some(ORIGIN),
            content_type: Some("application/json"),
            body,
            ..Self::get(path)
        }
    }

    fn signed(self, signed: &'a Signed) -> Self {
        Self {
            cookie: Some(&signed.cookie),
            csrf: Some(&signed.csrf),
            ..self
        }
    }
}

async fn send(app: &Router, call: Call<'_>) -> (StatusCode, HeaderMap, Value) {
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

async fn parts(response: Response) -> (StatusCode, HeaderMap, Value) {
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

fn set_cookies(headers: &HeaderMap) -> Vec<&str> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect()
}

/// Asserts the C03 envelope, that the request ID matches the header, and that the body holds no
/// user data.
fn assert_error(status: StatusCode, headers: &HeaderMap, body: &Value, expected: u16, code: &str) {
    assert_eq!(status.as_u16(), expected, "{body}");
    assert_eq!(body["error"]["code"], code);
    assert_eq!(
        body["error"]["requestId"].as_str().unwrap(),
        headers["x-request-id"].to_str().unwrap()
    );
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    assert!(!body.to_string().contains("example.test"));
    assert!(body.get("user").is_none());
}

#[sqlx::test]
async fn anonymous_bootstrap_issues_csrf_cookie_without_a_session(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let (status, headers, body) = send(&app, Call::get("/session")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).unwrap();
    assert_eq!(body["user"], Value::Null);
    let token = body["csrfToken"].as_str().unwrap().to_owned();
    let cookies = set_cookies(&headers);
    assert_eq!(cookies.len(), 1);
    assert!(cookies[0].starts_with("__Host-scenecask-csrf="));
    assert!(cookies[0].ends_with("; Path=/; HttpOnly; SameSite=Lax; Secure"));
    let secret = cookies[0]
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    assert_ne!(token, secret, "token must not reveal the cookie");

    // The bootstrap cookie keeps the token stable and is never an authenticated session.
    let anon_cookie = cookies[0].split(';').next().unwrap();
    let (_, headers, body) = send(
        &app,
        Call {
            cookie: Some(anon_cookie),
            ..Call::get("/session")
        },
    )
    .await;
    assert_eq!(body["csrfToken"], token);
    assert!(set_cookies(&headers).is_empty());
    let (status, headers, body) = send(
        &app,
        Call {
            cookie: Some(anon_cookie),
            ..Call::get("/probe/auth")
        },
    )
    .await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

#[sqlx::test]
async fn authenticated_session_returns_user_and_stores_only_a_hash(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let signed = sign_in(&security, ana).await;
    let (status, _, body) = send(
        &app,
        Call {
            cookie: Some(&signed.cookie),
            ..Call::get("/session")
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "user": {"id": ana, "email": "ana@example.test", "displayName": "Ana", "handle": null,
                     "visibility": "private", "verified": true, "role": "member"},
            "csrfToken": signed.csrf,
        })
    );
    let (hash, idle_ok, absolute_days): (Vec<u8>, bool, f64) = sqlx::query_as(
        "SELECT id_hash, last_seen_at >= created_at,
                (extract(epoch FROM expires_at - created_at) / 86400)::float8 FROM sessions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hash, signed.hash);
    assert!(idle_ok);
    assert!((absolute_days - 30.0).abs() < 0.001);
    let secret = signed.cookie.split_once('=').unwrap().1;
    assert_ne!(hash, secret.as_bytes());
}

#[sqlx::test]
async fn expired_revoked_or_disabled_sessions_return_401_and_lose_grants(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let expiries = [
        // Idle: last seen more than 7 days ago.
        "UPDATE sessions SET last_seen_at = now() - interval '7 days 1 second' WHERE id_hash = $1",
        // Absolute: created 30 days ago, still recently seen.
        "UPDATE sessions SET created_at = now() - interval '30 days 1 second',
             expires_at = now() - interval '1 second' WHERE id_hash = $1",
        // Revoked elsewhere.
        "DELETE FROM sessions WHERE id_hash = $1",
    ];
    for (index, expire) in expiries.iter().enumerate() {
        let id = user(&pool, &format!("user{index}@example.test"), true, "member").await;
        let signed = sign_in(&security, id).await;
        grant(&pool, &signed.hash, id).await;
        sqlx::query(sqlx::AssertSqlSafe(*expire))
            .bind(&signed.hash)
            .execute(&pool)
            .await
            .unwrap();
        let (status, headers, body) = send(&app, Call::get("/probe/auth").signed(&signed)).await;
        assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
        assert_eq!(
            set_cookies(&headers),
            ["__Host-scenecask=; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=0"]
        );
        assert_eq!(count(&pool, "sessions", &signed.hash).await, 0);
        assert_eq!(count(&pool, "reveal_grants", &signed.hash).await, 0);
        let (_, _, body) = send(
            &app,
            Call {
                cookie: Some(&signed.cookie),
                ..Call::get("/session")
            },
        )
        .await;
        assert_eq!(body["user"], Value::Null);
    }

    // Just inside both limits the session is still live.
    let id = user(&pool, "live@example.test", true, "member").await;
    let signed = sign_in(&security, id).await;
    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '29 days',
             last_seen_at = now() - interval '6 days 23 hours' WHERE id_hash = $1",
    )
    .bind(&signed.hash)
    .execute(&pool)
    .await
    .unwrap();
    let (status, _, _) = send(&app, Call::get("/probe/auth").signed(&signed)).await;
    assert_eq!(status, StatusCode::OK);

    // Disabling the account ends its sessions.
    sqlx::query("UPDATE users SET disabled_at = now() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, headers, body) = send(&app, Call::get("/probe/auth").signed(&signed)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 0);
}

#[sqlx::test]
async fn activity_slides_idle_expiry_at_most_once_per_interval(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let id = user(&pool, "ana@example.test", true, "member").await;
    let signed = sign_in(&security, id).await;
    let age = || async {
        sqlx::query_scalar::<_, f64>(
            "SELECT extract(epoch FROM now() - last_seen_at)::float8 FROM sessions",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '30 seconds'")
        .execute(&pool)
        .await
        .unwrap();
    send(&app, Call::get("/probe/auth").signed(&signed)).await;
    assert!(age().await >= 30.0, "recent activity is not rewritten");
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '6 days'")
        .execute(&pool)
        .await
        .unwrap();
    send(&app, Call::get("/probe/auth").signed(&signed)).await;
    assert!(age().await < 5.0, "older activity slides the idle window");
}

#[sqlx::test]
async fn mutations_without_valid_origin_csrf_or_json_are_rejected_without_writes(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let signed = sign_in(&security, ana).await;
    let other = sign_in(
        &security,
        user(&pool, "ben@example.test", true, "member").await,
    )
    .await;
    let (_, _, anonymous) = send(&app, Call::get("/session")).await;
    let anonymous_token = anonymous["csrfToken"].as_str().unwrap();
    let base = || Call::post("/probe/write", "").signed(&signed);
    let rejected = [
        Call {
            csrf: None,
            ..base()
        },
        Call {
            csrf: Some("wrong"),
            ..base()
        },
        Call {
            csrf: Some(&other.csrf),
            ..base()
        },
        Call {
            csrf: Some(anonymous_token),
            ..base()
        },
        Call {
            origin: None,
            ..base()
        },
        Call {
            origin: Some("https://evil.example"),
            ..base()
        },
        Call {
            origin: Some("null"),
            ..base()
        },
        Call {
            content_type: Some("text/plain"),
            body: "{}",
            ..base()
        },
        Call {
            content_type: None,
            body: "{}",
            ..base()
        },
        Call {
            content_type: Some("application/x-www-form-urlencoded"),
            body: "a=b",
            ..base()
        },
    ];
    for call in rejected {
        let (status, headers, body) = send(&app, call).await;
        assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    }
    let written: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
            .bind(ana)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(written.as_deref(), Some("Ana"));

    let (status, _, _) = send(&app, base()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let written: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
        .bind(ana)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(written, "Anaw");
}

#[sqlx::test]
async fn logout_revokes_session_and_grants_and_cannot_be_reused(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let signed = sign_in(&security, ana).await;
    let kept = sign_in(&security, ana).await;
    grant(&pool, &signed.hash, ana).await;
    grant(&pool, &kept.hash, ana).await;

    // Logout is a mutation: no token, no revocation.
    let (status, headers, body) = send(
        &app,
        Call {
            csrf: None,
            ..Call::post("/auth/logout", "{}").signed(&signed)
        },
    )
    .await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 1);

    let (status, headers, body) =
        send(&app, Call::post("/auth/logout", "{}").signed(&signed)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(
        set_cookies(&headers),
        ["__Host-scenecask=; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=0"]
    );
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 0);
    assert_eq!(count(&pool, "reveal_grants", &signed.hash).await, 0);
    // Other sessions of the same user are untouched.
    assert_eq!(count(&pool, "reveal_grants", &kept.hash).await, 1);

    // The old cookie and token are dead.
    let (status, headers, body) = send(&app, Call::post("/probe/write", "").signed(&signed)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    let (_, _, body) = send(
        &app,
        Call {
            cookie: Some(&signed.cookie),
            ..Call::get("/session")
        },
    )
    .await;
    assert_eq!(body["user"], Value::Null);

    // Already logged out is still 204; it still needs a same-origin JSON request.
    let (status, _, _) = send(&app, Call::post("/auth/logout", "{}").signed(&signed)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, Call::post("/auth/logout", "{}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, headers, body) = send(
        &app,
        Call {
            origin: Some("https://evil.example"),
            ..Call::post("/auth/logout", "{}").signed(&kept)
        },
    )
    .await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    assert_eq!(count(&pool, "sessions", &kept.hash).await, 1);
}

#[sqlx::test]
async fn logout_rejects_malformed_or_unknown_bodies_without_revoking(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let signed = sign_in(
        &security,
        user(&pool, "ana@example.test", true, "member").await,
    )
    .await;
    for (body, status, code) in [
        ("{", 400, "VALIDATION_ERROR"),
        (r#"{"all":true}"#, 422, "VALIDATION_ERROR"),
    ] {
        let (actual, headers, json) =
            send(&app, Call::post("/auth/logout", body).signed(&signed)).await;
        assert_error(actual, &headers, &json, status, code);
        assert!(
            !json.to_string().contains("all"),
            "parser text is not echoed"
        );
    }
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 1);
}

#[sqlx::test]
async fn rotation_and_user_revocation_remove_old_sessions_and_grants(pool: PgPool) {
    let security = security(&pool);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let old = sign_in(&security, ana).await;
    grant(&pool, &old.hash, ana).await;

    // Rotation needs the resolved current session, as login/verify handlers receive it. The probe
    // uses a plain connection, not a transaction, so ordering inside start_session is what keeps a
    // refused rotation from signing the caller out.
    let probe = Router::new()
        .route(
            "/rotate/{target}",
            post(
                |axum::extract::State(security): axum::extract::State<Security>,
                 axum::extract::Path(target): axum::extract::Path<Uuid>,
                 AuthUser(current): AuthUser| async move {
                    let mut conn = security.pool.acquire().await?;
                    let started =
                        start_session(&mut conn, &security.config, Some(&current), target).await?;
                    Ok::<_, ApiError>(started.cookie.to_str().unwrap().to_owned())
                },
            ),
        )
        .with_state(security.clone());
    let app = middleware::apply(probe, &security);
    let rotate = |target: Uuid| {
        Request::post(format!("/rotate/{target}"))
            .header(header::COOKIE, &old.cookie)
            .header("x-csrf-token", &old.csrf)
            .header(header::ORIGIN, ORIGIN)
            .body(Body::empty())
            .unwrap()
    };

    let pending = user(&pool, "pending@example.test", false, "member").await;
    let (status, headers, body) = parts(app.clone().oneshot(rotate(pending)).await.unwrap()).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    assert!(set_cookies(&headers).is_empty());
    assert_eq!(count(&pool, "sessions", &old.hash).await, 1);
    assert_eq!(count(&pool, "reveal_grants", &old.hash).await, 1);

    let response = app.oneshot(rotate(ana)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let current =
        String::from_utf8(to_bytes(response.into_body(), 1024).await.unwrap().to_vec()).unwrap();
    assert!(current.contains("; Max-Age=2592000"));
    assert_eq!(count(&pool, "sessions", &old.hash).await, 0);
    assert_eq!(count(&pool, "reveal_grants", &old.hash).await, 0);
    let fresh = signed(&current, String::new());
    assert_ne!(fresh.hash, old.hash);
    assert_eq!(count(&pool, "sessions", &fresh.hash).await, 1);

    let second = sign_in(&security, ana).await;
    grant(&pool, &second.hash, ana).await;
    assert_eq!(store::revoke_user(&pool, ana).await.unwrap(), 2);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM reveal_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test]
async fn reauthentication_rotates_the_secret_and_keeps_grants_and_expiry(pool: PgPool) {
    let security = security(&pool);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let old = sign_in(&security, ana).await;
    grant(&pool, &old.hash, ana).await;
    // An older sign-in whose last reauthentication is stale.
    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '10 days', expires_at = now() + interval '20 days',
             reauthenticated_at = now() - interval '10 days' WHERE id_hash = $1",
    )
    .bind(&old.hash)
    .execute(&pool)
    .await
    .unwrap();
    let expiry = || async {
        sqlx::query_scalar::<_, String>("SELECT expires_at::text FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let expires_before = expiry().await;

    let probe = Router::new()
        .route(
            "/reauth",
            post(
                |axum::extract::State(security): axum::extract::State<Security>,
                 AuthUser(current): AuthUser| async move {
                    let mut tx = security.pool.begin().await?;
                    let started = reauthenticate(&mut tx, &security.config, &current).await?;
                    tx.commit().await?;
                    Ok::<_, ApiError>(format!(
                        "{}\n{}",
                        started.cookie.to_str().unwrap(),
                        started.session.csrf_token
                    ))
                },
            ),
        )
        .route(
            "/fresh",
            get(|AuthUser(current): AuthUser| async move {
                current
                    .reauthenticated_within(std::time::Duration::from_secs(300))
                    .to_string()
            }),
        )
        .with_state(security.clone());
    let app = middleware::apply(probe, &security);
    let (_, _, body) = send(&app, Call::get("/fresh").signed(&old)).await;
    assert_eq!(
        body,
        Value::Bool(false),
        "reauthentication is stale before rotation"
    );
    let (status, _, body) = send(&app, Call::post("/reauth", "").signed(&old)).await;
    assert_eq!(status, StatusCode::OK);
    let (cookie, csrf) = body.as_str().unwrap().split_once('\n').unwrap();
    let max_age: u64 = cookie.rsplit_once("Max-Age=").unwrap().1.parse().unwrap();
    assert!(
        (20 * 86400 - 5..=20 * 86400).contains(&max_age),
        "{max_age}"
    );
    let fresh = signed(cookie, csrf.to_owned());
    assert_ne!(fresh.hash, old.hash);
    assert_ne!(fresh.csrf, old.csrf);

    // The old cookie is gone; the new one is live, fresh and keeps the grant and expiry.
    assert_eq!(count(&pool, "sessions", &old.hash).await, 0);
    assert_eq!(count(&pool, "reveal_grants", &fresh.hash).await, 1);
    assert_eq!(expiry().await, expires_before);
    let (status, headers, body) = send(&app, Call::get("/fresh").signed(&old)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    let (status, _, body) = send(&app, Call::get("/fresh").signed(&fresh)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, Value::Bool(true));
    // The new CSRF token is paired to the new session.
    let (status, _, _) = send(&app, Call::post("/reauth", "").signed(&fresh)).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn purge_removes_only_expired_sessions(pool: PgPool) {
    let security = security(&pool);
    let ana = user(&pool, "ana@example.test", true, "member").await;
    let live = sign_in(&security, ana).await;
    let idle = sign_in(&security, ana).await;
    grant(&pool, &idle.hash, ana).await;
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '8 days' WHERE id_hash = $1")
        .bind(&idle.hash)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store::purge_expired(&pool, &security.config.lifetimes)
            .await
            .unwrap(),
        1
    );
    assert_eq!(count(&pool, "sessions", &live.hash).await, 1);
    assert_eq!(count(&pool, "reveal_grants", &idle.hash).await, 0);
}

#[sqlx::test]
async fn guards_require_verified_email_and_operator_role(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let member = sign_in(
        &security,
        user(&pool, "member@example.test", true, "member").await,
    )
    .await;
    let operator = sign_in(
        &security,
        user(&pool, "operator@example.test", true, "operator").await,
    )
    .await;

    // start_session refuses unverified users; insert a legacy session directly to test the guard.
    let pending = user(&pool, "pending@example.test", false, "member").await;
    let mut conn = pool.acquire().await.unwrap();
    let error = start_session(&mut conn, &security.config, None, pending)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), ErrorCode::EmailUnverified);
    drop(conn);
    let pending_cookie = format!("{COOKIE}={}", "A".repeat(43));
    sqlx::query(
        "INSERT INTO sessions (id_hash, user_id, expires_at) VALUES ($1, $2, now() + interval '1 day')",
    )
    .bind(sha256(&"A".repeat(43)))
    .bind(pending)
    .execute(&pool)
    .await
    .unwrap();
    let call = |path, cookie| Call {
        cookie: Some(cookie),
        ..Call::get(path)
    };

    let (status, _, _) = send(&app, call("/probe/auth", &pending_cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, body) = send(&app, call("/probe/verified", &pending_cookie)).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    let (status, _, _) = send(&app, call("/probe/verified", &member.cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, body) = send(&app, call("/probe/operator", &member.cookie)).await;
    assert_error(status, &headers, &body, 403, "OPERATOR_REQUIRED");
    let (status, _, _) = send(&app, call("/probe/operator", &operator.cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, body) = send(&app, Call::get("/probe/operator")).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
}

#[sqlx::test]
async fn authenticated_writes_are_rate_limited_per_user(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let ana = sign_in(
        &security,
        user(&pool, "ana@example.test", true, "member").await,
    )
    .await;
    let ben = sign_in(
        &security,
        user(&pool, "ben@example.test", true, "member").await,
    )
    .await;
    for _ in 0..3 {
        let (status, _, _) = send(&app, Call::post("/probe/write", "").signed(&ana)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, headers, body) = send(&app, Call::post("/probe/write", "").signed(&ana)).await;
    assert_error(status, &headers, &body, 429, "RATE_LIMITED");
    let retry: u64 = headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry));
    // Reads are not write-limited, and other users keep their own budget.
    let (status, _, _) = send(&app, Call::get("/probe/auth").signed(&ana)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(&app, Call::post("/probe/write", "").signed(&ben)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Only the three permitted writes ran.
    let names: Vec<String> =
        sqlx::query_scalar("SELECT display_name FROM users ORDER BY normalized_email")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(names, ["Anawww", "Anaw"]);
}

#[sqlx::test]
async fn oversized_bodies_and_unknown_routes_use_the_envelope(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let signed = sign_in(
        &security,
        user(&pool, "ana@example.test", true, "member").await,
    )
    .await;
    let big = format!("{{\"padding\":\"{}\"}}", "x".repeat(2048));
    let (status, headers, body) =
        send(&app, Call::post("/auth/logout", &big).signed(&signed)).await;
    assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    assert_eq!(body["error"]["message"], "Check the request format.");
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 1);

    // A body without Content-Length (as when chunked) is capped by the extractor limit.
    let response = app
        .clone()
        .oneshot(
            Request::post("/auth/logout")
                .header(header::COOKIE, &signed.cookie)
                .header("x-csrf-token", &signed.csrf)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(big))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, headers, body) = parts(response).await;
    assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    assert_eq!(count(&pool, "sessions", &signed.hash).await, 1);

    let (status, headers, body) = send(&app, Call::get("/nope?token=secret-value")).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    // A wrong method on a known path also uses the envelope, not an empty 405.
    let (status, headers, body) = send(&app, Call::get("/auth/logout")).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let (status, headers, body) = send(
        &app,
        Call {
            method: Method::DELETE,
            ..Call::post("/session", "")
        },
    )
    .await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
}

#[sqlx::test]
#[traced_test]
async fn logs_never_contain_cookies_tokens_emails_or_query_strings(pool: PgPool) {
    let security = security(&pool);
    let app = app(&security);
    let signed = sign_in(
        &security,
        user(&pool, "ana@example.test", true, "member").await,
    )
    .await;
    send(
        &app,
        Call::get("/session?token=query-secret").signed(&signed),
    )
    .await;
    send(&app, Call::post("/probe/write", "").signed(&signed)).await;
    send(
        &app,
        Call {
            csrf: Some("bad-token"),
            ..Call::post("/auth/logout", "{}").signed(&signed)
        },
    )
    .await;
    send(&app, Call::post("/auth/logout", "{}").signed(&signed)).await;
    assert!(logs_contain("request completed"));
    let secret = signed.cookie.split_once('=').unwrap().1;
    for forbidden in [
        secret,
        signed.csrf.as_str(),
        "ana@example.test",
        "query-secret",
        "bad-token",
    ] {
        assert!(!logs_contain(forbidden));
    }
}
