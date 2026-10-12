//! R24 alpha integration through the production composition (`app::application`): every alpha
//! route is mounted under `/api/v1` behind the C03 layers, beta routes stay hidden, and the C09
//! personal-tracking journey holds with the metadata provider down, a concurrent edit before
//! Undo, and other/unverified sessions that must never receive protected payloads.

use axum::{
    Router,
    http::{Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, TestApp, TestUser, assert_error, released, seed_show, send, sign_in, tracking_state,
};

const ALPHA: &[(&str, &str)] = &[
    ("POST", "/auth/reauth"),
    ("PATCH", "/me"),
    ("GET", "/shows/search?q=hollow"),
    ("POST", "/shows/import"),
    ("GET", "/shows/{id}"),
    ("GET", "/shows/{id}/episodes"),
    ("GET", "/episodes/{id}"),
    ("GET", "/library"),
    ("PUT", "/library/{id}"),
    ("PATCH", "/library/{id}"),
    ("PUT", "/progress/episodes/{id}"),
    ("POST", "/shows/{id}/catch-up/preview"),
    ("POST", "/shows/{id}/catch-up"),
    ("DELETE", "/shows/{id}/history"),
    ("POST", "/actions/{id}/undo"),
    ("GET", "/home"),
    ("POST", "/reveals"),
    ("DELETE", "/reveals/episode_details/{id}"),
];

const BETA: &[(&str, &str)] = &[
    ("GET", "/people?q=ana"),
    ("POST", "/people/{id}/follow"),
    ("GET", "/me/follow-requests"),
    ("PATCH", "/me/privacy"),
    ("GET", "/activity"),
    ("GET", "/recommendations"),
    ("GET", "/episodes/{id}/discussions"),
    ("GET", "/notifications"),
    ("GET", "/operator/reports"),
    ("POST", "/me/export"),
];

fn path(template: &str) -> String {
    format!(
        "/api/v1{}",
        template.replace("{id}", &Uuid::new_v4().to_string())
    )
}

fn key() -> String {
    Uuid::new_v4().to_string()
}

async fn anonymous(app: &Router, method: &str, path: &str) -> (StatusCode, Value) {
    let method = Method::from_bytes(method.as_bytes()).unwrap();
    let call = if method == Method::GET {
        Call::get(path)
    } else {
        Call::write(method, path, "{}")
    };
    let (status, _, body) = send(app, call).await;
    (status, body)
}

#[sqlx::test]
async fn alpha_routes_are_guarded_and_beta_routes_are_absent(pool: PgPool) {
    let app = TestApp::new(&pool, |_| {}).application();
    let (status, _, body) = send(&app, Call::get("/api/v1/session")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["user"].is_null() && body["csrfToken"].is_string());
    for (method, template) in ALPHA {
        let (status, body) = anonymous(&app, method, &path(template)).await;
        assert_ne!(body["error"]["code"], "NOT_FOUND", "{method} {template}");
        assert!(
            matches!(status.as_u16(), 401 | 403),
            "{method} {template}: {status} {body}"
        );
    }
    for (method, template) in BETA {
        let (status, body) = anonymous(&app, method, &path(template)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {template}");
        assert_eq!(body["error"]["code"], "NOT_FOUND");
    }
    // Logging out without a session is an idempotent 204 (C04).
    assert_eq!(
        anonymous(&app, "POST", "/api/v1/auth/logout").await.0,
        StatusCode::NO_CONTENT
    );
    let (status, _, body) = send(&app, Call::get("/health/ready")).await;
    assert_eq!(
        (status, body["status"].as_str()),
        (StatusCode::OK, Some("ready"))
    );
}

struct Journey {
    app: Router,
    pool: PgPool,
    test: TestApp,
}

impl Journey {
    async fn get(&self, user: &TestUser, path: &str) -> (StatusCode, Value) {
        let path = format!("/api/v1{path}");
        let (status, headers, body) = send(&self.app, Call::get(&path).signed(&user.signed)).await;
        assert_eq!(headers["cache-control"], "private, no-store");
        (status, body)
    }

    async fn write(&self, user: &TestUser, method: Method, path: &str, body: Value) -> Value {
        let (status, body) = self.try_write(user, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        body
    }

    async fn try_write(
        &self,
        user: &TestUser,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let (path, body, key) = (format!("/api/v1{path}"), body.to_string(), key());
        let call = Call::write(method, &path, &body)
            .idempotent(&key)
            .signed(&user.signed);
        let (status, headers, body) = send(&self.app, call).await;
        if !status.is_success() {
            assert_error(
                status,
                &headers,
                &body,
                status.as_u16(),
                body["error"]["code"].as_str().unwrap(),
            );
        }
        (status, body)
    }

    async fn mark(&self, user: &TestUser, episode: Uuid, watched: bool, revision: i64) -> Value {
        let body = json!({"watched": watched, "expectedRevision": revision});
        let path = format!("/progress/episodes/{episode}");
        self.write(user, Method::PUT, &path, body).await
    }

    async fn watched(&self, user: &TestUser, show: Uuid) -> Vec<(i64, bool)> {
        let (status, body) = self
            .get(user, &format!("/shows/{show}/episodes?season=1"))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["number"].as_i64().unwrap(),
                    e["watched"].as_bool().unwrap(),
                )
            })
            .collect()
    }
}

/// A returning show: E1–E5 released, E6 announced for the future.
async fn journey(pool: PgPool) -> (Journey, Uuid, Vec<Uuid>) {
    let test = TestApp::new(&pool, |_| {});
    let app = test.application();
    let (show, episodes) = seed_show(
        &pool,
        "Hollow Orchard",
        "returning",
        &[
            released(1, 1),
            released(1, 2),
            released(1, 3),
            released(1, 4),
            released(1, 5),
            (1, 6, Some("2099-01-01")),
        ],
    )
    .await;
    (Journey { app, pool, test }, show, episodes)
}

#[sqlx::test]
async fn out_of_order_catch_up_and_undo_survive_provider_outage_and_concurrent_edits(pool: PgPool) {
    let (j, show, ep) = journey(pool).await;
    let ana = TestUser::create(&j.test.security, true, "member").await;

    // Provider down: search and import fail cleanly, with no write.
    let before = tracking_state(&j.pool).await;
    let (status, body) = j.get(&ana, "/shows/search?q=hollow").await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::BAD_GATEWAY, &json!("PROVIDER_UNAVAILABLE"))
    );
    let (status, _) = j
        .try_write(
            &ana,
            Method::POST,
            "/shows/import",
            json!({"providerId": 123}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(tracking_state(&j.pool).await, before);

    // Save from the cached catalog, watch E1, then E4 out of order: E2 stays next.
    let saved = j
        .write(
            &ana,
            Method::PUT,
            &format!("/library/{show}"),
            json!({"saved": true, "status": "plan_to_watch", "expectedRevision": 0}),
        )
        .await;
    assert_eq!(saved["library"]["status"], "plan_to_watch");
    j.mark(&ana, ep[0], true, 0).await;
    let marked = j.mark(&ana, ep[3], true, 0).await;
    assert_eq!(marked["library"]["status"], "watching");
    let (_, home) = j.get(&ana, "/home").await;
    let next = &home["upNext"][0]["progress"];
    assert_eq!(next["nextEpisode"]["id"], json!(ep[1]));
    assert_eq!(next["outOfOrder"], true);
    assert!(!home.to_string().contains("Protected"));
    let (_, e4) = j.get(&ana, &format!("/episodes/{}", ep[3])).await;
    assert_eq!(e4["detailsAccess"], "watched");
    assert_eq!(e4["details"]["title"], "Protected title");
    let (_, e2) = j.get(&ana, &format!("/episodes/{}", ep[1])).await;
    assert_eq!(e2["detailsAccess"], "locked");
    assert!(!e2.to_string().contains("Protected") && !e2.to_string().contains(".jpg"));

    // Explicit catch-up through E4 includes only the released, unwatched E2/E3.
    let preview = j
        .write(
            &ana,
            Method::POST,
            &format!("/shows/{show}/catch-up/preview"),
            json!({"throughEpisodeId": ep[3]}),
        )
        .await;
    assert_eq!(preview["count"], 2);
    let caught = j
        .write(
            &ana,
            Method::POST,
            &format!("/shows/{show}/catch-up"),
            json!({"previewId": preview["previewId"]}),
        )
        .await;
    assert_eq!(caught["changed"], 2);
    let all = [
        (1, true),
        (2, true),
        (3, true),
        (4, true),
        (5, false),
        (6, false),
    ];
    assert_eq!(j.watched(&ana, show).await, all);

    // Newer edits to E2 (unmark, then mark again) win over Undo, even with an equal value;
    // Undo reverts only E3.
    j.mark(&ana, ep[1], false, 1).await;
    j.mark(&ana, ep[1], true, 2).await;
    let action = caught["actionId"].as_str().unwrap();
    let undone = j
        .write(
            &ana,
            Method::POST,
            &format!("/actions/{action}/undo"),
            json!({}),
        )
        .await;
    assert_eq!(
        (undone["reverted"].as_i64(), undone["skipped"].as_i64()),
        (Some(1), Some(1))
    );
    let after = [
        (1, true),
        (2, true),
        (3, false),
        (4, true),
        (5, false),
        (6, false),
    ];
    assert_eq!(j.watched(&ana, show).await, after);

    // Logout revokes the session; a new sign-in reads the same authoritative state.
    let logout = Call::post("/api/v1/auth/logout", "{}").signed(&ana.signed);
    assert_eq!(send(&j.app, logout).await.0, StatusCode::NO_CONTENT);
    let (status, body) = j.get(&ana, "/home").await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::UNAUTHORIZED, &json!("AUTH_REQUIRED"))
    );
    let again = TestUser {
        id: ana.id,
        signed: sign_in(&j.test.security, ana.id).await,
    };
    assert_eq!(j.watched(&again, show).await, after);
    let (_, home) = j.get(&again, "/home").await;
    assert_eq!(
        home["upNext"][0]["progress"]["nextEpisode"]["id"],
        json!(ep[2])
    );
}

#[sqlx::test]
async fn other_sessions_and_unverified_accounts_never_receive_protected_payloads(pool: PgPool) {
    let (j, show, ep) = journey(pool).await;
    let security = &j.test.security;
    let ana = TestUser::create(security, true, "member").await;
    let marked = j.mark(&ana, ep[2], true, 0).await;
    let reveal = json!({"scope": "episode_details", "resourceId": ep[1]});
    let (status, _) = j.try_write(&ana, Method::POST, "/reveals", reveal).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, revealed) = j.get(&ana, &format!("/episodes/{}", ep[1])).await;
    assert_eq!(revealed["detailsAccess"], "revealed");

    // Ana's other session: the reveal belongs to the first session only.
    let other = TestUser {
        id: ana.id,
        signed: sign_in(security, ana.id).await,
    };
    let (_, e2) = j.get(&other, &format!("/episodes/{}", ep[1])).await;
    assert_eq!(e2["detailsAccess"], "locked");
    assert!(!e2.to_string().contains("Protected"));

    // Another member: no progress, no library, no unlocked details, cannot undo Ana's action.
    let bob = TestUser::create(security, true, "member").await;
    for path in [
        format!("/episodes/{}", ep[2]),
        format!("/shows/{show}"),
        format!("/shows/{show}/episodes?season=1"),
        "/library".to_owned(),
        "/home".to_owned(),
    ] {
        let (status, body) = j.get(&bob, &path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let text = body.to_string();
        assert!(
            !text.contains("Protected") && !text.contains("protected-episode"),
            "{path}"
        );
        assert!(!text.contains("\"watched\":true"), "{path}");
    }
    let action = marked["actionId"].as_str().unwrap();
    let (status, body) = j
        .try_write(
            &bob,
            Method::POST,
            &format!("/actions/{action}/undo"),
            json!({}),
        )
        .await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::NOT_FOUND, &json!("NOT_FOUND"))
    );

    // An unverified account is refused before any personal or catalog payload.
    let pending = TestUser::create(security, false, "member").await;
    for path in [
        format!("/episodes/{}", ep[2]),
        format!("/shows/{show}"),
        "/library".to_owned(),
        "/home".to_owned(),
    ] {
        let (status, body) = j.get(&pending, &path).await;
        assert_eq!(
            (status, &body["error"]["code"]),
            (StatusCode::FORBIDDEN, &json!("EMAIL_UNVERIFIED")),
            "{path}"
        );
        assert!(!body.to_string().contains("Hollow"));
    }

    // Logging out the revealing session removes its grants.
    let logout = Call::post("/api/v1/auth/logout", "{}").signed(&ana.signed);
    assert_eq!(send(&j.app, logout).await.0, StatusCode::NO_CONTENT);
    assert_eq!(
        common::count(&j.pool, "SELECT count(*) FROM reveal_grants").await,
        0
    );
}
