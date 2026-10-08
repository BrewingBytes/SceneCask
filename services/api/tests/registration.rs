//! R07 registration, verification and outbox delivery (C04) against real PostgreSQL. Delivery
//! uses recording/failing transports at the SMTP boundary, plus the local SMTP capture server for
//! the real transport. Expiry and cooldown are driven by moving rows back in time against the
//! database clock.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Router, http::StatusCode, routing::get};
use lettre::Message;
use scenecask_api::{
    mail::{MailTransport, SmtpMailer},
    middleware::Security,
    modules::{
        auth::{
            registration::{self, VERIFY_KIND, VerificationMail},
            session::VerifiedUser,
        },
        mail::{Pass, Worker, outbox},
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tracing_test::traced_test;
use uuid::Uuid;

mod common;
use common::{
    COOKIE, Call, ORIGIN, TestApp, TestUser, assert_error, count, send, set_cookies, sha256,
    signed, user,
};

const EMAIL: &str = "ana@example.test";
const PASSWORD: &str = "a-long-test-password";

fn app(pool: &PgPool) -> (Security, Router) {
    let harness = TestApp::new(pool, |_| {});
    let routes = registration::routes(harness.security.clone())
        .route("/probe/verified", get(|_: VerifiedUser| async { "ok" }));
    let app = harness.routes(routes);
    (harness.security, app)
}

fn register_body(email: &str, password: &str) -> String {
    json!({ "email": email, "password": password }).to_string()
}

async fn register(app: &Router, email: &str, password: &str) -> (StatusCode, Value) {
    let body = register_body(email, password);
    let (status, _, body) = send(app, Call::post("/auth/register", &body)).await;
    (status, body)
}

async fn resend(app: &Router, email: &str) -> (StatusCode, Value) {
    let body = json!({ "email": email }).to_string();
    let (status, _, body) = send(app, Call::post("/auth/verification-resend", &body)).await;
    (status, body)
}

async fn verify(app: &Router, token: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
    let body = json!({ "token": token }).to_string();
    send(app, Call::post("/auth/verify", &body)).await
}

fn check_email() -> Value {
    json!({ "message": "Check your email." })
}

/// Asserts the generic, enumeration-safe 202.
fn assert_generic(response: (StatusCode, Value)) {
    assert_eq!(response, (StatusCode::ACCEPTED, check_email()));
}

/// Records each sent message as its formatted RFC 5322 text.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn sent(&self) -> Vec<String> {
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
struct Down;

impl MailTransport for Down {
    async fn send(&self, _: Message) -> Result<(), &'static str> {
        Err("email delivery unavailable")
    }
}

fn worker<T: MailTransport + Sync>(pool: &PgPool, transport: T) -> Worker<T, VerificationMail> {
    let composer = VerificationMail::new(
        pool.clone(),
        ORIGIN.to_owned(),
        "SceneCask <no-reply@scenecask.test>".parse().unwrap(),
    );
    Worker::new(pool.clone(), transport, composer)
}

/// Sends every due message through a fresh recorder and returns the recorded messages.
async fn deliver(pool: &PgPool) -> Vec<String> {
    let recorder = Recorder::default();
    worker(pool, recorder.clone()).run_once().await.unwrap();
    recorder.sent()
}

/// Undoes the quoted-printable soft line breaks and `=3D` escapes lettre uses for long lines.
fn decoded(message: &str) -> String {
    message.replace("=\r\n", "").replace("=3D", "=")
}

/// The token from the verification link (`…/auth/verify#token=<43 base64url chars>`).
fn token_in(message: &str) -> String {
    let message = decoded(message);
    let marker = format!("{ORIGIN}/auth/verify#token=");
    let start = message.find(&marker).expect("verification link") + marker.len();
    let token: String = message[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert_eq!(token.len(), 43, "token must be a full 256-bit secret");
    token
}

async fn user_id(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn password_hash(pool: &PgPool, user_id: Uuid) -> String {
    sqlx::query_scalar("SELECT argon2_hash FROM password_credentials WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every stored auth/outbox value as text, to prove no plaintext secret was persisted.
async fn stored_text(pool: &PgPool) -> String {
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

/// Moves this user's verification tokens `seconds` into the past.
async fn age_tokens(pool: &PgPool, user_id: Uuid, seconds: i64) {
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

async fn make_due(pool: &PgPool) {
    sqlx::query("UPDATE outbox SET available_at = now() WHERE delivered_at IS NULL")
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn new_email_queues_one_usable_link_and_stores_no_plaintext_secret(pool: PgPool) {
    let (_, app) = app(&pool);
    let (status, body) = register(&app, "  Ana@Example.TEST ", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, check_email());

    // Trimmed and casefolded; pending, with an Argon2id hash and no session.
    let id = user_id(&pool, EMAIL).await;
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM users WHERE verified_at IS NULL"
        )
        .await,
        1
    );
    let hash = password_hash(&pool, id).await;
    assert!(hash.starts_with("$argon2id$v=19$"));
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);

    // One queued email whose payload is only the account ID; no token exists before sending.
    let payload: Value = sqlx::query_scalar("SELECT payload FROM outbox WHERE kind = $1")
        .bind(VERIFY_KIND)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(payload, json!({ "userId": id }));
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM auth_tokens").await, 0);

    let sent = deliver(&pool).await;
    assert_eq!(sent.len(), 1);
    let message = &decoded(&sent[0]);
    assert!(message.contains("To: ana@example.test"));
    assert!(message.contains("Subject: Verify your SceneCask email"));
    assert!(message.contains("multipart/alternative"));
    assert!(message.contains("text/plain") && message.contains("text/html"));
    let token = token_in(message);
    // The link appears in both parts, plus the layout's footer with the configured origin.
    assert_eq!(
        message.matches(&token).count(),
        3,
        "plain link, html href and html text"
    );
    assert!(message.contains(&format!("SceneCask account at {ORIGIN}.")));

    // Only the token's hash is stored; neither the token nor the password appear anywhere.
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM auth_tokens WHERE purpose = 'verify'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, sha256(&token));
    let text = stored_text(&pool).await;
    assert!(!text.contains(&token));
    assert!(!text.contains(PASSWORD));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE delivered_at IS NOT NULL"
        )
        .await,
        1
    );

    // Nothing left to send.
    assert!(deliver(&pool).await.is_empty());
}

#[sqlx::test]
async fn verification_consumes_the_link_rotates_the_session_and_unlocks_the_library(pool: PgPool) {
    let (security, app) = app(&pool);
    // An unrelated session presented with the request is rotated away.
    let other = TestUser::create(&security, true, "member").await;
    register(&app, EMAIL, PASSWORD).await;
    let token = token_in(&deliver(&pool).await[0]);

    let body = json!({ "token": token }).to_string();
    let (status, headers, body) = send(
        &app,
        Call::post("/auth/verify", &body).signed(&other.signed),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = user_id(&pool, EMAIL).await;
    assert_eq!(body["user"]["id"], json!(id));
    assert_eq!(body["user"]["email"], EMAIL);
    assert_eq!(body["user"]["verified"], true);
    assert_eq!(body["user"]["role"], "member");
    let cookies = set_cookies(&headers);
    assert_eq!(cookies.len(), 1);
    assert!(cookies[0].starts_with(&format!("{COOKIE}=")));
    let session = signed(cookies[0], body["csrfToken"].as_str().unwrap().to_owned());
    assert_eq!(
        count(&pool, "SELECT count(*) FROM sessions").await,
        1,
        "the presented session was revoked"
    );
    assert_eq!(
        common::count_session(&pool, "sessions", &other.signed.hash).await,
        0
    );

    // The new session reaches verified-only APIs.
    let (status, _, _) = send(
        &app,
        Call {
            cookie: Some(&session.cookie),
            ..Call::get("/probe/verified")
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Single use: a replay fails with the resend recovery and changes nothing.
    let (status, headers, body) = verify(&app, &token).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert!(set_cookies(&headers).is_empty());
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 1);
}

#[sqlx::test]
async fn pending_accounts_cannot_reach_the_library(pool: PgPool) {
    let (security, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
    // Even a session that predates a pending state is refused (R06 guard).
    let pending = TestUser::create(&security, false, "member").await;
    let (status, headers, body) = send(
        &app,
        Call {
            cookie: Some(&pending.signed.cookie),
            ..Call::get("/probe/verified")
        },
    )
    .await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
}

#[sqlx::test]
async fn expired_link_fails_410_and_preserves_state(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    let token = token_in(&deliver(&pool).await[0]);
    let id = user_id(&pool, EMAIL).await;
    age_tokens(&pool, id, 24 * 60 * 60).await;

    let (status, headers, body) = verify(&app, &token).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM users WHERE verified_at IS NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM auth_tokens WHERE consumed_at IS NULL"
        )
        .await,
        1
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);

    // Recovery: resend sends a fresh link that works.
    resend(&app, EMAIL).await;
    let fresh = token_in(&deliver(&pool).await[0]);
    let (status, _, _) = verify(&app, &fresh).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn malformed_and_unknown_tokens(pool: PgPool) {
    let (_, app) = app(&pool);
    let (status, headers, body) = verify(&app, "").await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert_eq!(
        body["error"]["fields"]["token"],
        "Use the link from your email."
    );
    for token in ["not-a-token", &"A".repeat(43)] {
        let (status, headers, body) = verify(&app, token).await;
        assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    }
    let (status, headers, body) = send(
        &app,
        Call::post("/auth/verify", r#"{"token":"x","extra":1}"#),
    )
    .await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
}

#[sqlx::test]
async fn resend_invalidates_the_old_link_and_honours_the_cooldown(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    let first = token_in(&deliver(&pool).await[0]);
    let id = user_id(&pool, EMAIL).await;

    // Within 60 seconds of the last email: same answer, nothing queued, old link still valid.
    assert_generic(resend(&app, EMAIL).await);
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 1);

    age_tokens(&pool, id, 61).await;
    assert_generic(resend(&app, EMAIL).await);
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 2);
    // The old link stops working as soon as the resend is accepted, before the new one is sent.
    let (status, headers, body) = verify(&app, &first).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");

    let second = token_in(&deliver(&pool).await[0]);
    assert_ne!(first, second);
    let (status, _, _) = verify(&app, &second).await;
    assert_eq!(status, StatusCode::OK);

    // Verified accounts get the generic answer and no email.
    age_tokens(&pool, id, 61).await;
    assert_generic(resend(&app, EMAIL).await);
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 2);
}

#[sqlx::test]
async fn existing_accounts_are_enumeration_safe_and_unchanged(pool: PgPool) {
    let (_, app) = app(&pool);
    let verified = user(&pool, "bo@example.test", true, "member").await;
    sqlx::query(
        "INSERT INTO password_credentials (user_id, argon2_hash) VALUES ($1, '$argon2id$x')",
    )
    .bind(verified)
    .execute(&pool)
    .await
    .unwrap();
    let disabled = user(&pool, "cy@example.test", false, "member").await;
    sqlx::query("UPDATE users SET disabled_at = now() WHERE id = $1")
        .bind(disabled)
        .execute(&pool)
        .await
        .unwrap();

    let new = register(&app, EMAIL, PASSWORD).await;
    for email in [
        "bo@example.test",
        "BO@example.test",
        "cy@example.test",
        EMAIL,
    ] {
        assert_eq!(register(&app, email, "another-long-password").await, new);
    }
    for email in ["bo@example.test", "cy@example.test", "nobody@example.test"] {
        assert_eq!(resend(&app, email).await, new);
    }
    // One account per email; verified and disabled accounts are untouched and get no email.
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 3);
    assert_eq!(password_hash(&pool, verified).await, "$argon2id$x");
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 1);
    // The pending account's re-registration was inside its cooldown, so its password stands.
    let pending = user_id(&pool, EMAIL).await;
    let original = password_hash(&pool, pending).await;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM password_credentials").await,
        2
    );

    // After the cooldown, re-registering a pending account sends a new link and the earlier one
    // stops working, but a link was already minted, so a later registrant cannot swap in their
    // own password: the newest link still activates the original one.
    let first = token_in(&deliver(&pool).await[0]);
    age_tokens(&pool, pending, 61).await;
    register(&app, EMAIL, "the-newest-long-password").await;
    assert_eq!(password_hash(&pool, pending).await, original);
    let (status, headers, body) = verify(&app, &first).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    let second = token_in(&deliver(&pool).await[0]);
    assert_eq!(verify(&app, &second).await.0, StatusCode::OK);
    assert_eq!(password_hash(&pool, pending).await, original);
}

#[sqlx::test]
async fn concurrent_duplicate_signups_create_one_account_and_one_email(pool: PgPool) {
    let (_, app) = app(&pool);
    let attempts = (0..5).map(|i| {
        let app = app.clone();
        let email = if i % 2 == 0 {
            EMAIL
        } else {
            " ANA@example.test"
        };
        tokio::spawn(async move { register(&app, email, PASSWORD).await })
    });
    for attempt in attempts {
        assert_eq!(
            attempt.await.unwrap(),
            (StatusCode::ACCEPTED, check_email())
        );
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 1);
}

#[sqlx::test]
#[traced_test]
async fn smtp_outage_is_invisible_and_retries_without_duplicates(pool: PgPool) {
    let (_, app) = app(&pool);
    assert_generic(register(&app, EMAIL, PASSWORD).await);

    let pass = worker(&pool, Down).run_once().await.unwrap();
    assert_eq!(
        pass,
        Pass {
            sent: 0,
            skipped: 0,
            failed: 1
        }
    );
    let (attempts, retry_in): (i32, f64) = sqlx::query_as(
        "SELECT attempts, extract(epoch FROM available_at - now())::float8 FROM outbox",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 1);
    assert!((25.0..=31.0).contains(&retry_in), "backoff {retry_in}");
    // Not due yet: another pass does nothing.
    assert_eq!(
        worker(&pool, Down).run_once().await.unwrap(),
        Pass::default()
    );

    // During the outage, repeated signup and resend answer the same and queue nothing more.
    assert_generic(register(&app, EMAIL, PASSWORD).await);
    assert_generic(resend(&app, EMAIL).await);
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 1);

    // Recovery delivers exactly once; only the delivered link is usable.
    make_due(&pool).await;
    let sent = deliver(&pool).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM auth_tokens WHERE consumed_at IS NULL"
        )
        .await,
        1
    );
    assert!(deliver(&pool).await.is_empty());
    assert_eq!(verify(&app, &token_in(&sent[0])).await.0, StatusCode::OK);

    // Logs name the kind and category only.
    assert!(logs_contain("outbox delivery failed"));
    assert!(logs_contain(VERIFY_KIND));
    assert!(!logs_contain("example.test"));
    assert!(!logs_contain(PASSWORD));
}

#[sqlx::test]
async fn crashed_worker_lease_is_recovered(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    // A worker claims the message and dies before settling it.
    let claimed = outbox::claim(&pool, 10, outbox::LEASE).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert!(
        deliver(&pool).await.is_empty(),
        "leased messages are not reclaimed early"
    );
    // The lease ends; another worker delivers it, and the crashed claim counts as an attempt.
    make_due(&pool).await;
    assert_eq!(deliver(&pool).await.len(), 1);
    assert_eq!(count(&pool, "SELECT attempts::bigint FROM outbox").await, 2);
}

#[sqlx::test]
async fn a_taken_over_claim_cannot_settle_or_delay_the_message(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    // A slow worker's lease ends and another worker takes the message over.
    let stale = outbox::claim(&pool, 1, outbox::LEASE)
        .await
        .unwrap()
        .remove(0);
    make_due(&pool).await;
    let current = outbox::claim(&pool, 1, outbox::LEASE)
        .await
        .unwrap()
        .remove(0);
    assert_eq!((stale.id, current.attempts), (current.id, 2));
    // The stale holder finishing late changes nothing; the current holder settles it.
    outbox::settle(&pool, &stale).await.unwrap();
    outbox::retry_later(&pool, &stale, Duration::ZERO)
        .await
        .unwrap();
    let pending = "SELECT count(*) FROM outbox WHERE delivered_at IS NULL AND available_at > now()";
    assert_eq!(count(&pool, pending).await, 1);
    outbox::settle(&pool, &current).await.unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE delivered_at IS NULL"
        )
        .await,
        0
    );
}

#[sqlx::test]
async fn each_message_is_leased_just_before_its_own_send(pool: PgPool) {
    let (_, app) = app(&pool);
    for n in 0..3 {
        register(&app, &format!("user{n}@example.test"), PASSWORD).await;
    }
    // While the first message is being sent, the rest of the batch is still unclaimed.
    let unclaimed = "SELECT count(*) FROM outbox WHERE attempts = 0";
    let observed = Arc::new(Mutex::new(Vec::new()));
    let probe = Probe(pool.clone(), Arc::clone(&observed), unclaimed);
    assert_eq!(worker(&pool, probe).run_once().await.unwrap().sent, 3);
    assert_eq!(*observed.lock().unwrap(), [2, 1, 0]);
}

/// Records, at each send, how many rows `sql` counts.
struct Probe(PgPool, Arc<Mutex<Vec<i64>>>, &'static str);

impl MailTransport for Probe {
    async fn send(&self, _: Message) -> Result<(), &'static str> {
        let seen = count(&self.0, self.2).await;
        self.1.lock().unwrap().push(seen);
        Ok(())
    }
}

#[sqlx::test]
#[traced_test]
async fn retries_are_bounded_and_an_exhausted_email_allows_a_resend(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    sqlx::query("UPDATE outbox SET attempts = $1")
        .bind(outbox::MAX_ATTEMPTS - 1)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(worker(&pool, Down).run_once().await.unwrap().failed, 1);
    assert!(logs_contain("outbox message exhausted its retries"));
    make_due(&pool).await;
    assert_eq!(
        worker(&pool, Down).run_once().await.unwrap(),
        Pass::default()
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE delivered_at IS NULL"
        )
        .await,
        1
    );

    // The dead message no longer blocks recovery once the cooldown from its link has passed.
    age_tokens(&pool, user_id(&pool, EMAIL).await, 61).await;
    resend(&app, EMAIL).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM outbox").await, 2);
    make_due(&pool).await;
    assert_eq!(deliver(&pool).await.len(), 1);
}

#[sqlx::test]
async fn queued_email_is_skipped_once_the_account_no_longer_needs_it(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    sqlx::query("UPDATE users SET verified_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let pass = worker(&pool, Recorder::default()).run_once().await.unwrap();
    assert_eq!(
        pass,
        Pass {
            sent: 0,
            skipped: 1,
            failed: 0
        }
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM auth_tokens").await, 0);
}

#[sqlx::test]
async fn disabled_account_link_fails_without_a_session(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    let token = token_in(&deliver(&pool).await[0]);
    sqlx::query("UPDATE users SET disabled_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let (status, headers, body) = verify(&app, &token).await;
    assert_error(status, &headers, &body, 410, "TOKEN_EXPIRED");
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM users WHERE verified_at IS NULL"
        )
        .await,
        1
    );
}

#[sqlx::test]
async fn validation_errors_name_fields_without_echoing_input(pool: PgPool) {
    let (_, app) = app(&pool);
    for (email, password, fields) in [
        ("not-an-email", PASSWORD, vec!["email"]),
        (EMAIL, "short", vec!["password"]),
        (EMAIL, &"p".repeat(129), vec!["password"]),
        ("", "", vec!["email", "password"]),
    ] {
        let body = register_body(email, password);
        let (status, headers, body) = send(&app, Call::post("/auth/register", &body)).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
        let named: Vec<&str> = body["error"]["fields"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(named, fields);
    }
    // Length counts characters and the password is never trimmed: 12 with surrounding spaces.
    assert_eq!(
        register(&app, EMAIL, "  ten chars ").await.0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        register(&app, "b@example.test", &"é".repeat(128)).await.0,
        StatusCode::ACCEPTED
    );

    for (path, body) in [
        (
            "/auth/register",
            r#"{"email":"ana@example.test","password":"a-long-test-password","role":"operator"}"#,
        ),
        (
            "/auth/verification-resend",
            r#"{"email":"ana@example.test","extra":true}"#,
        ),
        ("/auth/verification-resend", r#"{"email":"nope"}"#),
    ] {
        let (status, headers, body) = send(&app, Call::post(path, body)).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    }
    let (status, headers, body) = send(&app, Call::post("/auth/register", "{not json")).await;
    assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 2);
}

#[sqlx::test]
async fn cross_origin_requests_are_refused_before_any_write(pool: PgPool) {
    let (_, app) = app(&pool);
    let body = register_body(EMAIL, PASSWORD);
    for origin in [None, Some("https://evil.example")] {
        let (status, headers, body) = send(
            &app,
            Call {
                origin,
                ..Call::post("/auth/register", &body)
            },
        )
        .await;
        assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 0);
}

#[sqlx::test]
async fn auth_rate_limit_applies_per_identifier(pool: PgPool) {
    let (_, app) = app(&pool);
    for _ in 0..5 {
        assert_eq!(resend(&app, EMAIL).await.0, StatusCode::ACCEPTED);
    }
    let body = json!({ "email": " ANA@example.test" }).to_string();
    let (status, headers, body) = send(&app, Call::post("/auth/verification-resend", &body)).await;
    assert_error(status, &headers, &body, 429, "RATE_LIMITED");
    assert!(headers.contains_key("retry-after"));
    // Another identifier has its own budget.
    assert_eq!(
        resend(&app, "bo@example.test").await.0,
        StatusCode::ACCEPTED
    );
}

#[sqlx::test]
#[traced_test]
async fn logs_never_contain_credentials_tokens_or_addresses(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, EMAIL, PASSWORD).await;
    let token = token_in(&deliver(&pool).await[0]);
    verify(&app, &token).await;
    verify(&app, &token).await;
    for secret in [EMAIL, PASSWORD, token.as_str()] {
        assert!(!logs_contain(secret));
    }
}

#[sqlx::test]
async fn delivers_through_local_smtp_capture(pool: PgPool) {
    let host = std::env::var("SMTP_HOST").expect("SMTP_HOST required for local capture test");
    let port = std::env::var("SMTP_PORT")
        .expect("SMTP_PORT required")
        .parse()
        .unwrap();
    let (_, app) = app(&pool);
    register(
        &app,
        &format!("r07-{}@scenecask.test", Uuid::new_v4()),
        PASSWORD,
    )
    .await;
    let pass = worker(&pool, SmtpMailer::local_capture(&host, port))
        .run_once()
        .await
        .unwrap();
    assert_eq!(
        pass,
        Pass {
            sent: 1,
            skipped: 0,
            failed: 0
        }
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE delivered_at IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM auth_tokens").await, 1);

    // An unreachable relay is a retryable failure, not a lost message.
    register(
        &app,
        &format!("r07-{}@scenecask.test", Uuid::new_v4()),
        PASSWORD,
    )
    .await;
    let pass = worker(&pool, SmtpMailer::local_capture("127.0.0.1", 1))
        .run_once()
        .await
        .unwrap();
    assert_eq!(
        pass,
        Pass {
            sent: 0,
            skipped: 0,
            failed: 1
        }
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE delivered_at IS NULL"
        )
        .await,
        1
    );
}

#[sqlx::test]
async fn email_normalization_is_the_database_full_casefold(pool: PgPool) {
    let (_, app) = app(&pool);
    register(&app, "Straße@example.test", PASSWORD).await;
    register(&app, "STRASSE@EXAMPLE.TEST", PASSWORD).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    user_id(&pool, "strasse@example.test").await;
    // The normalized lookup keeps the column collation, so it uses the unique index.
    let plan: Vec<String> = sqlx::query_scalar(
        r#"EXPLAIN SELECT id FROM users
           WHERE normalized_email = casefold($1 COLLATE pg_unicode_fast) COLLATE "default""#,
    )
    .bind("Ana@example.test")
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        plan.join("\n").contains("users_normalized_email_key"),
        "{plan:?}"
    );
}
