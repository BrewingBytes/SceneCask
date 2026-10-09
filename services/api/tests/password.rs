//! R08 password login, recovery and fresh reauthentication (C04) against real PostgreSQL. Reset
//! email goes through the outbox to a recording transport; expiry, cooldown and freshness are
//! driven by moving rows back in time against the database clock.

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, phc::PasswordHash},
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use scenecask_api::{
    middleware::Security,
    modules::auth::{
        email_token::RESET,
        password::{self, REAUTH_WINDOW},
        session::{self, AuthUser, VerifiedUser},
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tracing_test::traced_test;
use uuid::Uuid;

mod common;
use common::{
    COOKIE, Call, Signed, TestApp, age_tokens, assert_error, assert_generic, count, count_session,
    deliver, disable, grant, link_token, make_due, password_hash, send, set_cookies, sign_in,
    signed, stale_reauth, stored_text, until_blocked, user,
};

const EMAIL: &str = "ana@example.test";
const PASSWORD: &str = "a-long-test-password";
const NEW_PASSWORD: &str = "another-long-password";
const RESET_PATH: &str = "/auth/reset/confirm";

fn app(pool: &PgPool) -> (Security, Router) {
    let harness = TestApp::new(pool, |_| {});
    let routes = password::routes(harness.security.clone())
        .merge(session::routes(harness.security.clone()))
        .route("/probe/verified", get(|_: VerifiedUser| async { "ok" }))
        .route(
            "/probe/fresh",
            get(|AuthUser(current): AuthUser| async move {
                current.reauthenticated_within(REAUTH_WINDOW).to_string()
            }),
        );
    let app = harness.routes(routes);
    (harness.security, app)
}

fn argon2(password: &str) -> String {
    PasswordHasher::<PasswordHash>::hash_password(&Argon2::default(), password.as_bytes())
        .unwrap()
        .to_string()
}

/// An account with `password` (and none when `None`, as for Google-only sign-in).
async fn account(pool: &PgPool, email: &str, password: Option<&str>, verified: bool) -> Uuid {
    let id = user(pool, email, verified, "member").await;
    if let Some(password) = password {
        sqlx::query("INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, $2)")
            .bind(id)
            .bind(argon2(password))
            .execute(pool)
            .await
            .unwrap();
    }
    id
}

async fn login_as(
    app: &Router,
    signed: Option<&Signed>,
    email: &str,
    password: &str,
) -> (StatusCode, HeaderMap, Value) {
    let body = json!({ "email": email, "password": password }).to_string();
    let mut call = Call::post("/auth/login", &body);
    if let Some(signed) = signed {
        call = call.signed(signed);
    }
    send(app, call).await
}

async fn login(app: &Router, email: &str, password: &str) -> (StatusCode, HeaderMap, Value) {
    login_as(app, None, email, password).await
}

async fn request_reset(app: &Router, email: &str) -> (StatusCode, Value) {
    let body = json!({ "email": email }).to_string();
    let (status, _, body) = send(app, Call::post("/auth/password-reset-request", &body)).await;
    (status, body)
}

async fn reset_as(
    app: &Router,
    signed: Option<&Signed>,
    token: &str,
    new_password: &str,
) -> (StatusCode, HeaderMap, Value) {
    let body = json!({ "token": token, "newPassword": new_password }).to_string();
    let mut call = Call::post("/auth/password-reset", &body);
    if let Some(signed) = signed {
        call = call.signed(signed);
    }
    send(app, call).await
}

async fn reset(app: &Router, token: &str, new_password: &str) -> (StatusCode, HeaderMap, Value) {
    reset_as(app, None, token, new_password).await
}

async fn reauth(app: &Router, signed: &Signed, password: &str) -> (StatusCode, HeaderMap, Value) {
    let body = json!({ "password": password }).to_string();
    send(app, Call::post("/auth/reauth", &body).signed(signed)).await
}

async fn probe(app: &Router, path: &str, cookie: &str) -> (StatusCode, Value) {
    let mut call = Call::get(path);
    call.cookie = Some(cookie);
    let (status, _, body) = send(app, call).await;
    (status, body)
}

/// Requests a reset for `email` and returns the token from the delivered link.
async fn reset_token(app: &Router, pool: &PgPool, email: &str) -> String {
    assert_generic(request_reset(app, email).await);
    let sent = deliver(pool).await;
    assert_eq!(sent.len(), 1);
    link_token(&sent[0], RESET_PATH)
}

/// The app and an account with a password, plus a delivered reset link for it.
async fn with_reset_link(pool: &PgPool) -> (Router, Uuid, String) {
    let (_, app) = app(pool);
    let id = account(pool, EMAIL, Some(PASSWORD), true).await;
    let token = reset_token(&app, pool, EMAIL).await;
    (app, id, token)
}

/// The session cookie from a response that set one.
fn session_cookie(headers: &HeaderMap) -> String {
    let cookies = set_cookies(headers);
    let set = cookies
        .iter()
        .find(|cookie| cookie.starts_with(&format!("{COOKIE}=")) && !cookie.contains("Max-Age=0"))
        .expect("session cookie");
    set.split(';').next().unwrap().to_owned()
}

fn clears_session(headers: &HeaderMap) -> bool {
    set_cookies(headers)
        .iter()
        .any(|cookie| cookie.starts_with(&format!("{COOKIE}=")) && cookie.contains("Max-Age=0"))
}

async fn sessions(pool: &PgPool, user_id: Uuid) -> i64 {
    count(
        pool,
        &format!("SELECT count(*) FROM sessions WHERE user_id = '{user_id}'"),
    )
    .await
}

async fn reset_emails(pool: &PgPool) -> i64 {
    count(
        pool,
        &format!("SELECT count(*) FROM outbox WHERE kind = '{}'", RESET.kind),
    )
    .await
}

#[sqlx::test]
async fn verified_credentials_return_a_rotated_session(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let previous = sign_in(&security, id).await;

    // Trimmed and casefolded like registration.
    let (status, headers, body) =
        login_as(&app, Some(&previous), "  Ana@Example.TEST ", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["id"], id.to_string());
    assert_eq!(body["user"]["verified"], true);
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    let cookie = session_cookie(&headers);
    assert_ne!(cookie, previous.cookie);
    assert!(!body["csrfToken"].as_str().unwrap().is_empty());

    // The carried session was rotated away; the new one reaches verified APIs and is fresh.
    assert_eq!(count_session(&pool, "sessions", &previous.hash).await, 0);
    assert_eq!(sessions(&pool, id).await, 1);
    assert_eq!(
        probe(&app, "/probe/verified", &previous.cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        probe(&app, "/probe/verified", &cookie).await,
        (StatusCode::OK, Value::String("ok".into()))
    );
    assert_eq!(
        probe(&app, "/probe/fresh", &cookie).await.1,
        Value::Bool(true)
    );
}

#[sqlx::test]
async fn invalid_credentials_are_generic_and_create_no_session(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), true).await;
    account(&pool, "google@example.test", None, true).await;
    let disabled = account(&pool, "gone@example.test", Some(PASSWORD), true).await;
    disable(&pool, disabled).await;

    let mut bodies = Vec::new();
    for (email, password) in [
        (EMAIL, "a-long-test-passworD"),
        // Never trimmed.
        (EMAIL, " a-long-test-password"),
        ("nobody@example.test", PASSWORD),
        ("google@example.test", PASSWORD),
        ("gone@example.test", PASSWORD),
    ] {
        let (status, headers, mut body) = login(&app, email, password).await;
        assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");
        assert!(set_cookies(&headers).is_empty());
        body["error"]["requestId"] = Value::Null;
        bodies.push(body);
    }
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn unverified_correct_credentials_route_to_verification_without_a_session(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), false).await;

    let (status, headers, body) = login(&app, EMAIL, PASSWORD).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    assert!(set_cookies(&headers).is_empty());
    // A wrong password does not reveal that the account is pending.
    let (status, headers, body) = login(&app, EMAIL, "not-the-password").await;
    assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn login_validation_and_rate_limits(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), true).await;

    for (email, password, fields) in [
        ("not-an-email", PASSWORD, vec!["email"]),
        (EMAIL, "", vec!["password"]),
        ("", &"x".repeat(129), vec!["email", "password"]),
    ] {
        let (status, headers, body) = login(&app, email, password).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
        let named: Vec<_> = body["error"]["fields"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(named, fields);
        assert!(!body.to_string().contains("xxxx"));
    }
    // 128 characters is a valid submission, just a wrong password.
    let (status, headers, body) = login(&app, EMAIL, &"x".repeat(128)).await;
    assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");

    // C03: 5 attempts per minute per IP and identifier; another identifier keeps its budget.
    for _ in 0..4 {
        assert_eq!(
            login(&app, EMAIL, "wrong-password").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, headers, body) = login(&app, "ANA@example.test", PASSWORD).await;
    assert_error(status, &headers, &body, 429, "RATE_LIMITED");
    assert!(headers.contains_key(header::RETRY_AFTER));
    assert_eq!(
        login(&app, "other@example.test", PASSWORD).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn reset_request_is_enumeration_safe(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), true).await;
    let disabled = account(&pool, "gone@example.test", Some(PASSWORD), true).await;
    disable(&pool, disabled).await;

    assert_generic(request_reset(&app, "nobody@example.test").await);
    assert_generic(request_reset(&app, "gone@example.test").await);
    assert_eq!(reset_emails(&pool).await, 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 2);

    assert_generic(request_reset(&app, " ANA@example.test").await);
    assert_eq!(reset_emails(&pool).await, 1);
    let (status, headers, body) = {
        let body = json!({ "email": "nope" }).to_string();
        send(&app, Call::post("/auth/password-reset-request", &body)).await
    };
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
}

#[sqlx::test]
async fn completed_reset_revokes_sessions_and_the_token(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let here = sign_in(&security, id).await;
    let elsewhere = sign_in(&security, id).await;
    grant(&pool, &elsewhere.hash, id).await;
    let other_user = account(&pool, "other@example.test", Some(PASSWORD), true).await;
    let unrelated = sign_in(&security, other_user).await;

    let token = reset_token(&app, &pool, EMAIL).await;
    assert!(!stored_text(&pool).await.contains(&token));
    let (status, headers, body) = reset_as(&app, Some(&here), &token, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(body, Value::Null);
    assert!(clears_session(&headers));

    // Every session and reveal grant of the account is gone; others are untouched.
    assert_eq!(sessions(&pool, id).await, 0);
    assert_eq!(
        count_session(&pool, "reveal_grants", &elsewhere.hash).await,
        0
    );
    assert_eq!(
        probe(&app, "/probe/verified", &elsewhere.cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        probe(&app, "/probe/verified", &unrelated.cookie).await.0,
        StatusCode::OK
    );

    // Replay fails and changes nothing; the old password is gone, the new one works.
    let stored = password_hash(&pool, id).await;
    let (status, headers, body) = reset(&app, &token, "a-third-long-password").await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert_eq!(password_hash(&pool, id).await, stored);
    let (status, headers, body) = login(&app, EMAIL, PASSWORD).await;
    assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");
    assert_eq!(login(&app, EMAIL, NEW_PASSWORD).await.0, StatusCode::OK);
}

#[sqlx::test]
async fn expired_reset_fails_410_and_preserves_state(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let kept = sign_in(&security, id).await;
    let token = reset_token(&app, &pool, EMAIL).await;
    let stored = password_hash(&pool, id).await;

    // Still valid just inside 30 minutes is covered by the other tests; past it fails.
    age_tokens(&pool, id, 30 * 60).await;
    let (status, headers, body) = reset(&app, &token, NEW_PASSWORD).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert_eq!(password_hash(&pool, id).await, stored);
    assert_eq!(count_session(&pool, "sessions", &kept.hash).await, 1);

    for token in ["", "not base64!", "AAAA"] {
        let (status, headers, body) = reset(&app, token, NEW_PASSWORD).await;
        let expected = if token.is_empty() { 422 } else { 410 };
        let code = if token.is_empty() {
            "VALIDATION_ERROR"
        } else {
            "TOKEN_EXPIRED"
        };
        assert_error(status, &headers, &body, expected, code);
    }
}

#[sqlx::test]
async fn a_newer_reset_request_invalidates_the_previous_link(pool: PgPool) {
    let (_, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let first = reset_token(&app, &pool, EMAIL).await;

    // Within the cooldown a repeat request sends nothing new.
    assert_generic(request_reset(&app, EMAIL).await);
    assert!(deliver(&pool).await.is_empty());
    assert_eq!(reset_emails(&pool).await, 1);

    age_tokens(&pool, id, 61).await;
    let second = reset_token(&app, &pool, EMAIL).await;
    assert_ne!(first, second);
    let (status, headers, body) = reset(&app, &first, NEW_PASSWORD).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert_eq!(
        reset(&app, &second, NEW_PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn concurrent_resets_with_one_token_succeed_once(pool: PgPool) {
    let (app, id, token) = with_reset_link(&pool).await;

    let (first, second) = tokio::join!(
        reset(&app, &token, NEW_PASSWORD),
        reset(&app, &token, "a-third-long-password"),
    );
    let mut statuses = [first.0.as_u16(), second.0.as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [204, 410]);
    let winner = if first.0 == StatusCode::NO_CONTENT {
        NEW_PASSWORD
    } else {
        "a-third-long-password"
    };
    assert_eq!(login(&app, EMAIL, winner).await.0, StatusCode::OK);
    assert_eq!(sessions(&pool, id).await, 1);
}

/// Runs `attempt`, a request that checks the old password, while a reset of `user_id` is in
/// progress as `store::reset` applies it: the new password is written, and only once `attempt`
/// finishes or waits on that write are all sessions revoked and the reset committed.
async fn racing_a_reset(
    pool: &PgPool,
    user_id: Uuid,
    attempt: impl Future<Output = (StatusCode, HeaderMap, Value)> + Send + 'static,
) {
    let mut reset = pool.begin().await.unwrap();
    sqlx::query("UPDATE password_credentials SET argon2_hash = $2 WHERE user_id = $1")
        .bind(user_id)
        .bind(argon2(NEW_PASSWORD))
        .execute(&mut *reset)
        .await
        .unwrap();
    // The old password still verifies against the committed hash.
    let attempt = tokio::spawn(attempt);
    until_blocked(pool, &attempt).await;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *reset)
        .await
        .unwrap();
    reset.commit().await.unwrap();

    let (status, headers, body) = attempt.await.unwrap();
    assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");
    assert_eq!(sessions(pool, user_id).await, 0);
}

#[sqlx::test]
async fn a_login_checked_against_the_old_password_never_outlives_a_reset(pool: PgPool) {
    let (_, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    racing_a_reset(&pool, id, async move { login(&app, EMAIL, PASSWORD).await }).await;
}

#[sqlx::test]
async fn a_reauth_checked_against_the_old_password_never_outlives_a_reset(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let current = sign_in(&security, id).await;
    racing_a_reset(
        &pool,
        id,
        async move { reauth(&app, &current, PASSWORD).await },
    )
    .await;
}

#[sqlx::test]
async fn new_password_boundaries(pool: PgPool) {
    let (app, id, token) = with_reset_link(&pool).await;

    for invalid in ["x".repeat(11), "é".repeat(129)] {
        let (status, headers, body) = reset(&app, &token, &invalid).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
        assert!(body["error"]["fields"]["newPassword"].is_string());
        assert!(!body.to_string().contains(&invalid));
    }
    // Rejected requests did not consume the link; 128 characters counted as characters, with
    // surrounding spaces kept.
    let longest = format!(" {} ", "é".repeat(126));
    assert_eq!(
        reset(&app, &token, &longest).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(login(&app, EMAIL, &longest).await.0, StatusCode::OK);
    assert_eq!(
        login(&app, EMAIL, longest.trim()).await.0,
        StatusCode::UNAUTHORIZED
    );

    age_tokens(&pool, id, 61).await;
    let token = reset_token(&app, &pool, EMAIL).await;
    let shortest = "x".repeat(12);
    assert_eq!(
        reset(&app, &token, &shortest).await.0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn reset_token_exchange_is_rate_limited(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), true).await;
    let token = reset_token(&app, &pool, EMAIL).await;
    for _ in 0..5 {
        assert_eq!(
            reset(&app, &token, "short").await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    // Validation failures happen before the limiter, so the token still has its budget.
    for _ in 0..5 {
        assert_eq!(reset(&app, "AAAA", NEW_PASSWORD).await.0, StatusCode::GONE);
    }
    let (status, headers, body) = reset(&app, "AAAA", NEW_PASSWORD).await;
    assert_error(status, &headers, &body, 429, "RATE_LIMITED");
    assert_eq!(
        reset(&app, &token, NEW_PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );

    // One request was made by `reset_token`; four more use up the minute.
    for _ in 0..4 {
        assert_generic(request_reset(&app, EMAIL).await);
    }
    let (status, headers, body) = {
        let body = json!({ "email": EMAIL }).to_string();
        send(&app, Call::post("/auth/password-reset-request", &body)).await
    };
    assert_error(status, &headers, &body, 429, "RATE_LIMITED");
}

#[sqlx::test]
async fn google_only_and_pending_accounts_can_establish_a_password(pool: PgPool) {
    let (_, app) = app(&pool);
    let google = account(&pool, "google@example.test", None, true).await;
    sqlx::query(
        "INSERT INTO external_identities (user_id, issuer, subject)
         VALUES ($1, 'https://accounts.google.com', 'subject-1')",
    )
    .bind(google)
    .execute(&pool)
    .await
    .unwrap();
    let token = reset_token(&app, &pool, "google@example.test").await;
    assert_eq!(
        reset(&app, &token, NEW_PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        login(&app, "google@example.test", NEW_PASSWORD).await.0,
        StatusCode::OK
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM external_identities").await,
        1
    );

    // A pending account proves its mailbox with the reset link, so it ends up verified.
    let pending = account(&pool, EMAIL, Some(PASSWORD), false).await;
    let token = reset_token(&app, &pool, EMAIL).await;
    assert_eq!(
        reset(&app, &token, NEW_PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*) FROM users WHERE id = '{pending}' AND verified_at IS NOT NULL"
            )
        )
        .await,
        1
    );
    assert_eq!(login(&app, EMAIL, NEW_PASSWORD).await.0, StatusCode::OK);
}

#[sqlx::test]
async fn disabled_account_link_fails_and_queued_email_is_skipped(pool: PgPool) {
    let (app, id, token) = with_reset_link(&pool).await;
    let queued = account(&pool, "queued@example.test", Some(PASSWORD), true).await;
    assert_generic(request_reset(&app, "queued@example.test").await);
    disable(&pool, id).await;
    disable(&pool, queued).await;
    let (status, headers, body) = reset(&app, &token, NEW_PASSWORD).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    make_due(&pool).await;
    assert!(deliver(&pool).await.is_empty());
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM auth_tokens WHERE purpose = 'reset'"
        )
        .await,
        1
    );
}

#[sqlx::test]
async fn reauth_rotates_the_session_and_marks_it_fresh(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let old = sign_in(&security, id).await;
    grant(&pool, &old.hash, id).await;
    stale_reauth(&pool, id).await;
    assert_eq!(
        probe(&app, "/probe/fresh", &old.cookie).await.1,
        Value::Bool(false)
    );

    let (status, headers, body) = reauth(&app, &old, PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let cookie = session_cookie(&headers);
    assert_ne!(cookie, old.cookie);
    assert_eq!(
        probe(&app, "/probe/fresh", &cookie).await.1,
        Value::Bool(true)
    );
    // The copied old cookie cannot inherit freshness; grants follow the session.
    assert_eq!(
        probe(&app, "/probe/fresh", &old.cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(sessions(&pool, id).await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM reveal_grants").await, 1);

    // The rotated session's CSRF token comes from GET /session.
    let (_, _, session) = send(&app, {
        let mut call = Call::get("/session");
        call.cookie = Some(&cookie);
        call
    })
    .await;
    let fresh = signed(
        &format!("{cookie};"),
        session["csrfToken"].as_str().unwrap().to_owned(),
    );
    assert_eq!(
        reauth(&app, &fresh, PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn reauth_failures_keep_the_session_unchanged(pool: PgPool) {
    let (security, app) = app(&pool);
    let id = account(&pool, EMAIL, Some(PASSWORD), true).await;
    let current = sign_in(&security, id).await;
    sqlx::query("UPDATE sessions SET reauthenticated_at = NULL WHERE user_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, headers, body) = reauth(&app, &current, "not-the-password").await;
    assert_error(status, &headers, &body, 401, "INVALID_CREDENTIALS");
    assert!(set_cookies(&headers).is_empty());
    let (status, headers, body) = reauth(&app, &current, "").await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert_eq!(
        probe(&app, "/probe/fresh", &current.cookie).await,
        (StatusCode::OK, Value::Bool(false))
    );

    // Anonymous and missing-CSRF requests never reach the password check.
    let body = json!({ "password": PASSWORD }).to_string();
    let (status, headers, response) = send(&app, Call::post("/auth/reauth", &body)).await;
    assert_error(status, &headers, &response, 401, "AUTH_REQUIRED");
    let mut call = Call::post("/auth/reauth", &body).signed(&current);
    call.csrf = None;
    let (status, headers, response) = send(&app, call).await;
    assert_error(status, &headers, &response, 403, "CSRF_FAILED");

    // Google-only accounts get the same generic 401 and use the Google reauth intent.
    let google = account(&pool, "google@example.test", None, true).await;
    let google = sign_in(&security, google).await;
    let (status, headers, response) = reauth(&app, &google, PASSWORD).await;
    assert_error(status, &headers, &response, 401, "INVALID_CREDENTIALS");

    // Limited per account: four more failures exhaust the minute's five checked attempts.
    for _ in 0..4 {
        assert_eq!(
            reauth(&app, &current, "not-the-password").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, headers, response) = reauth(&app, &current, PASSWORD).await;
    assert_error(status, &headers, &response, 429, "RATE_LIMITED");
}

#[sqlx::test]
#[traced_test]
async fn credentials_and_tokens_never_reach_logs(pool: PgPool) {
    let (_, app) = app(&pool);
    account(&pool, EMAIL, Some(PASSWORD), true).await;
    let token = reset_token(&app, &pool, EMAIL).await;
    login(&app, EMAIL, "not-the-password").await;
    reset(&app, &token, NEW_PASSWORD).await;
    login(&app, EMAIL, NEW_PASSWORD).await;
    for secret in [
        PASSWORD,
        NEW_PASSWORD,
        "not-the-password",
        token.as_str(),
        EMAIL,
    ] {
        assert!(!logs_contain(secret));
    }
}
