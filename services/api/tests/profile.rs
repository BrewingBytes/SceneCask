//! R15 `PATCH /me` evidence (C04) against migrated PostgreSQL: validation, initial-handle
//! immutability, uniqueness, and that role and visibility cannot be written.

use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use scenecask_api::modules::account::profile;
use serde_json::{Value, json};
use sqlx::PgPool;

mod common;
use common::{Call, TestApp, TestUser, assert_error, send};

struct Fixture {
    app: Router,
    test: TestApp,
    pool: PgPool,
}

async fn fixture(pool: PgPool) -> Fixture {
    let test = TestApp::new(&pool, |_| {});
    Fixture {
        app: test.routes(profile::routes(test.security.clone())),
        test,
        pool,
    }
}

impl Fixture {
    async fn patch(&self, user: &TestUser, body: Value) -> (StatusCode, HeaderMap, Value) {
        let body = body.to_string();
        send(
            &self.app,
            Call::write(Method::PATCH, "/me", &body).signed(&user.signed),
        )
        .await
    }

    async fn profile(&self, user: &TestUser) -> (Option<String>, Option<String>, String, String) {
        sqlx::query_as("SELECT display_name, handle, role, visibility FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

#[sqlx::test]
async fn sets_display_name_and_initial_handle(pool: PgPool) {
    let f = fixture(pool).await;
    let ana = TestUser::create(&f.test.security, true, "member").await;
    let (status, headers, body) = f
        .patch(&ana, json!({"displayName": "  Ana R  ", "handle": "ana_r"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(body["id"], json!(ana.id));
    assert_eq!(body["displayName"], "Ana R");
    assert_eq!(body["handle"], "ana_r");
    assert_eq!(body["visibility"], "private");
    assert_eq!(body["verified"], true);
    assert_eq!(body["role"], "member");
    assert!(body["email"].as_str().unwrap().ends_with("@example.test"));
    assert_eq!(
        f.profile(&ana).await,
        (
            Some("Ana R".into()),
            Some("ana_r".into()),
            "member".into(),
            "private".into()
        )
    );

    // The same handle may be resent with a new name; a different one is rejected.
    let (status, _, body) = f
        .patch(&ana, json!({"displayName": "Ana", "handle": "ana_r"}))
        .await;
    assert_eq!(
        (status, &body["displayName"]),
        (StatusCode::OK, &json!("Ana"))
    );
    let (status, headers, body) = f
        .patch(&ana, json!({"displayName": "Ana", "handle": "ana_two"}))
        .await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert!(body["error"]["fields"]["handle"].is_string());
    assert_eq!(f.profile(&ana).await.1.as_deref(), Some("ana_r"));
}

#[sqlx::test]
async fn handle_collision_is_409_and_changes_nothing(pool: PgPool) {
    let f = fixture(pool).await;
    let ana = TestUser::create(&f.test.security, true, "member").await;
    let ben = TestUser::create(&f.test.security, true, "member").await;
    f.patch(&ana, json!({"displayName": "Ana", "handle": "orchard"}))
        .await;
    let before = f.profile(&ben).await;
    let (status, headers, body) = f
        .patch(
            &ben,
            json!({"displayName": "Ben Changed", "handle": "orchard"}),
        )
        .await;
    assert_error(status, &headers, &body, 409, "HANDLE_TAKEN");
    assert!(body["error"]["fields"]["handle"].is_string());
    assert!(
        !body.to_string().contains("orchard"),
        "errors never echo submitted values"
    );
    assert_eq!(f.profile(&ben).await, before);
}

#[sqlx::test]
async fn invalid_fields_and_role_or_visibility_writes_are_rejected(pool: PgPool) {
    let f = fixture(pool).await;
    let ana = TestUser::create(&f.test.security, true, "member").await;
    let before = f.profile(&ana).await;
    for (body, fields) in [
        (
            json!({"displayName": "Ana", "handle": "Ana"}),
            vec!["handle"],
        ),
        (
            json!({"displayName": "Ana", "handle": "an"}),
            vec!["handle"],
        ),
        (
            json!({"displayName": "Ana", "handle": "a".repeat(31)}),
            vec!["handle"],
        ),
        (
            json!({"displayName": "   ", "handle": "ana-r"}),
            vec!["displayName", "handle"],
        ),
        (
            json!({"displayName": "x".repeat(81), "handle": "ana_r"}),
            vec!["displayName"],
        ),
        (
            json!({"displayName": "Ana", "handle": "ana_r", "role": "operator"}),
            vec![],
        ),
        (
            json!({"displayName": "Ana", "handle": "ana_r", "visibility": "public"}),
            vec![],
        ),
        (json!({"displayName": "Ana"}), vec![]),
    ] {
        let (status, headers, response) = f.patch(&ana, body.clone()).await;
        assert_error(status, &headers, &response, 422, "VALIDATION_ERROR");
        for field in fields {
            assert!(
                response["error"]["fields"][field].is_string(),
                "{body} {field}"
            );
        }
    }
    assert_eq!(f.profile(&ana).await, before);

    // An operator cannot use the profile endpoint to change roles either way.
    let operator = TestUser::create(&f.test.security, true, "operator").await;
    let (status, headers, body) = f
        .patch(
            &operator,
            json!({"displayName": "Op", "handle": "op", "role": "member"}),
        )
        .await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert_eq!(f.profile(&operator).await.2, "operator");
}

#[sqlx::test]
async fn profile_requires_a_verified_session_and_csrf(pool: PgPool) {
    let f = fixture(pool).await;
    let pending = TestUser::create(&f.test.security, false, "member").await;
    let request = json!({"displayName": "Ana", "handle": "ana_r"});
    let (status, headers, body) = f.patch(&pending, request.clone()).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");

    let ana = TestUser::create(&f.test.security, true, "member").await;
    let text = request.to_string();
    let mut call = Call::write(Method::PATCH, "/me", &text).signed(&ana.signed);
    call.csrf = None;
    let (status, headers, body) = send(&f.app, call).await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    let (status, headers, body) = send(&f.app, Call::write(Method::PATCH, "/me", &text)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    assert_eq!(f.profile(&ana).await.1, None);
}
