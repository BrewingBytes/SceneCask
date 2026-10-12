//! R25 social relationship evidence (C06) against migrated PostgreSQL: the follow matrix for
//! private and public accounts, request approval/decline, unfollow and follower removal revoking
//! library access immediately, deduplicated notifications and outbox events, concurrent writes,
//! private search leakage, cursors and concealed blocked lookups.

use axum::http::{Method, StatusCode};
use scenecask_api::modules::social;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, TestUser, assert_error, checked, count, disable, member_fixture, released,
    seed_show, send,
};

async fn fixture(pool: PgPool) -> MemberFixture {
    let f = member_fixture(pool, |security| social::routes(security.clone())).await;
    profile(&f.pool, f.ana.id, "ana_r", "Ana Rusu", "private").await;
    f
}

async fn profile(pool: &PgPool, id: Uuid, handle: &str, name: &str, visibility: &str) {
    sqlx::query("UPDATE users SET handle = $2, display_name = $3, visibility = $4 WHERE id = $1")
        .bind(id)
        .bind(handle)
        .bind(name)
        .bind(visibility)
        .execute(pool)
        .await
        .unwrap();
}

async fn block(pool: &PgPool, blocker: Uuid, blocked: Uuid) {
    sqlx::query("INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)")
        .bind(blocker)
        .bind(blocked)
        .execute(pool)
        .await
        .unwrap();
}

/// Saves a show with one watched episode to `user`'s library.
async fn save_show(pool: &PgPool, user: Uuid, title: &str) -> Uuid {
    let (show, episodes) =
        seed_show(pool, title, "returning", &[released(1, 1), released(1, 2)]).await;
    sqlx::query(
        "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
         VALUES ($1, $2, true, 'watching', clock_timestamp(), 3)",
    )
    .bind(user)
    .bind(show)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO episode_progress (user_id, episode_id, watched, revision) VALUES ($1, $2, true, 1)",
    )
    .bind(user)
    .bind(episodes[0])
    .execute(pool)
    .await
    .unwrap();
    show
}

impl MemberFixture {
    async fn member(&self, handle: &str, visibility: &str) -> TestUser {
        let user = TestUser::create(&self.test.security, true, "member").await;
        profile(&self.pool, user.id, handle, "Bo Ilie", visibility).await;
        user
    }

    async fn get(&self, user: &TestUser, path: &str) -> (StatusCode, Value) {
        checked(&self.app, Call::get(path).signed(&user.signed)).await
    }

    async fn write(
        &self,
        user: &TestUser,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let body = body.to_string();
        checked(
            &self.app,
            Call::write(method, path, &body).signed(&user.signed),
        )
        .await
    }

    async fn follow(&self, user: &TestUser, target: Uuid) -> (StatusCode, Value) {
        self.write(
            user,
            Method::POST,
            &format!("/people/{target}/follow"),
            json!({}),
        )
        .await
    }

    async fn unfollow(&self, user: &TestUser, target: Uuid) -> StatusCode {
        self.write(
            user,
            Method::DELETE,
            &format!("/people/{target}/follow"),
            json!({}),
        )
        .await
        .0
    }

    async fn decide(&self, user: &TestUser, requester: Uuid, decision: &str) -> StatusCode {
        self.write(
            user,
            Method::POST,
            &format!("/me/follow-requests/{requester}"),
            json!({ "decision": decision }),
        )
        .await
        .0
    }

    async fn library(&self, user: &TestUser, subject: Uuid) -> StatusCode {
        self.get(user, &format!("/people/{subject}/library"))
            .await
            .0
    }

    async fn notifications(&self) -> (i64, i64) {
        (
            count(&self.pool, "SELECT count(*) FROM notifications").await,
            count(
                &self.pool,
                "SELECT count(*) FROM outbox WHERE kind = 'notification.follow_request'",
            )
            .await,
        )
    }
}

#[sqlx::test]
async fn private_follow_stays_pending_and_hides_the_library_until_approved(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    save_show(&f.pool, bo.id, "Hollow Orchard").await;

    assert_eq!(
        f.follow(&f.ana, bo.id).await,
        (StatusCode::OK, json!({"state": "pending"}))
    );
    // A repeat is idempotent and cannot duplicate the request's notification or event.
    assert_eq!(
        f.follow(&f.ana, bo.id).await,
        (StatusCode::OK, json!({"state": "pending"}))
    );
    assert_eq!(f.notifications().await, (1, 1));
    let payload: Value = sqlx::query_scalar("SELECT payload FROM outbox")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let notification: Uuid = sqlx::query_scalar("SELECT id FROM notifications")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(payload, json!({ "notificationId": notification }));

    let (status, body) = f.get(&f.ana, &format!("/people/{}", bo.id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "person": {"id": bo.id, "handle": "bo_i", "displayName": "Bo Ilie", "initials": "BI",
                       "visibility": "private",
                       "relationship": {"outgoing": "pending", "incoming": "none", "mutual": false}},
            "canViewLibrary": false,
        })
    );
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::NOT_FOUND);

    let (status, body) = f.get(&bo, "/me/follow-requests").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["id"], json!(f.ana.id));
    assert_eq!(body["items"][0]["relationship"]["incoming"], "pending");
    assert_eq!(body["nextCursor"], Value::Null);

    assert_eq!(
        f.decide(&bo, f.ana.id, "approve").await,
        StatusCode::NO_CONTENT
    );
    // Approval is not repeatable: the request is no longer pending.
    assert_eq!(
        f.decide(&bo, f.ana.id, "approve").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.follow(&f.ana, bo.id).await,
        (StatusCode::OK, json!({"state": "approved"}))
    );
    let (_, body) = f.get(&f.ana, &format!("/people/{}", bo.id)).await;
    assert_eq!(body["canViewLibrary"], true);

    let (status, body) = f.get(&f.ana, &format!("/people/{}/library", bo.id)).await;
    assert_eq!(status, StatusCode::OK);
    let item = &body["items"][0];
    assert_eq!(item["title"], "Hollow Orchard");
    assert_eq!(item["status"], "watching");
    assert_eq!(
        item["progress"]["watched"], 1,
        "the subject's progress, not the viewer's"
    );
    assert!(item.get("revision").is_none(), "no internal revisions");
    assert!(!body.to_string().contains("Protected"), "no episode text");

    let (_, body) = f.get(&bo, "/me/followers").await;
    assert_eq!(body["items"][0]["id"], json!(f.ana.id));
    assert_eq!(
        f.get(&bo, "/me/follow-requests").await.1["items"],
        json!([])
    );
    // Approval sends nothing further.
    assert_eq!(f.notifications().await.1, 1);
}

#[sqlx::test]
async fn public_follow_is_approved_immediately_but_anonymous_access_is_denied(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    save_show(&f.pool, bo.id, "Hollow Orchard").await;
    let stranger = f.member("cy_stranger", "private").await;

    // Any signed-in member sees a public library, follower or not.
    assert_eq!(f.library(&stranger, bo.id).await, StatusCode::OK);
    assert_eq!(
        f.follow(&f.ana, bo.id).await,
        (StatusCode::OK, json!({"state": "approved"}))
    );
    assert_eq!(
        f.notifications().await,
        (0, 0),
        "no request, no notification"
    );
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::OK);

    for path in [
        format!("/people/{}/library", bo.id),
        format!("/people/{}", bo.id),
        "/people?q=bo_i".to_owned(),
    ] {
        let (status, headers, body) = send(&f.app, Call::get(&path)).await;
        assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    }
    let path = format!("/people/{}/follow", bo.id);
    let (status, _, body) = send(&f.app, Call::write(Method::POST, &path, "{}")).await;
    assert!([401, 403].contains(&status.as_u16()), "{body}");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM follows").await, 1);
}

#[sqlx::test]
async fn pending_request_survives_a_switch_to_public(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    f.follow(&f.ana, bo.id).await;
    sqlx::query("UPDATE users SET visibility = 'public' WHERE id = $1")
        .bind(bo.id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.follow(&f.ana, bo.id).await,
        (StatusCode::OK, json!({"state": "pending"}))
    );
}

#[sqlx::test]
async fn decline_remove_and_unfollow_revoke_access_immediately_and_notify_nobody(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    save_show(&f.pool, bo.id, "Hollow Orchard").await;

    // Decline: the edge and its notification go; nothing is queued.
    f.follow(&f.ana, bo.id).await;
    assert_eq!(
        f.decide(&bo, f.ana.id, "decline").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.decide(&bo, f.ana.id, "decline").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.notifications().await, (0, 1));
    assert_eq!(count(&f.pool, "SELECT count(*) FROM follows").await, 0);
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::NOT_FOUND);

    // A new request after a decline notifies once more.
    f.follow(&f.ana, bo.id).await;
    assert_eq!(f.notifications().await, (1, 2));
    f.decide(&bo, f.ana.id, "approve").await;
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::OK);

    // Follower removal: silent, idempotent and immediate.
    let path = format!("/me/followers/{}", f.ana.id);
    for _ in 0..2 {
        let (status, _) = f.write(&bo, Method::DELETE, &path, json!({})).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::NOT_FOUND);
    assert_eq!(f.notifications().await, (0, 2));

    // Unfollow, with Bo also following Ana: Ana's edge goes, Bo's stays approved.
    f.follow(&f.ana, bo.id).await;
    f.decide(&bo, f.ana.id, "approve").await;
    f.follow(&bo, f.ana.id).await;
    f.decide(&f.ana, bo.id, "approve").await;
    let (_, body) = f.get(&f.ana, &format!("/people/{}", bo.id)).await;
    assert_eq!(
        body["person"]["relationship"],
        json!({"outgoing": "approved", "incoming": "approved", "mutual": true})
    );
    let outbox = f.notifications().await.1;
    for _ in 0..2 {
        assert_eq!(f.unfollow(&f.ana, bo.id).await, StatusCode::NO_CONTENT);
    }
    assert_eq!(f.library(&f.ana, bo.id).await, StatusCode::NOT_FOUND);
    assert_eq!(f.library(&bo, f.ana.id).await, StatusCode::OK);
    let (_, body) = f.get(&f.ana, &format!("/people/{}", bo.id)).await;
    assert_eq!(
        body["person"]["relationship"],
        json!({"outgoing": "none", "incoming": "approved", "mutual": false})
    );
    assert_eq!(f.notifications().await.1, outbox);

    // Withdrawing a pending request removes its notification too.
    f.follow(&f.ana, bo.id).await;
    assert_eq!(f.notifications().await.0, 2);
    f.unfollow(&f.ana, bo.id).await;
    assert_eq!(f.notifications().await.0, 1);
    assert_eq!(
        f.get(&bo, "/me/follow-requests").await.1["items"],
        json!([])
    );
}

/// A discussion hosted by `host` with a one-hour discussion reveal grant on each session of
/// `viewer`.
async fn revealed_discussion(pool: &PgPool, host: Uuid, viewer: Uuid) {
    let (_, episodes) = seed_show(pool, "Thread show", "returning", &[released(1, 1)]).await;
    let discussion: Uuid = sqlx::query_scalar(
        "INSERT INTO discussions (host_id, episode_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(host)
    .bind(episodes[0])
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
         SELECT id_hash, user_id, 'discussion', $2, now() + interval '1 hour'
         FROM sessions WHERE user_id = $1",
    )
    .bind(viewer)
    .bind(discussion)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn unfollow_and_follower_removal_revoke_the_pairs_discussion_grants(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let cy = f.member("cy_t", "public").await;
    let grants = || count(&f.pool, "SELECT count(*) FROM reveal_grants");
    let mutual = async |a: &TestUser, b: &TestUser| {
        f.follow(a, b.id).await;
        f.follow(b, a.id).await;
        f.decide(a, b.id, "approve").await;
        f.decide(b, a.id, "approve").await;
    };
    mutual(&f.ana, &bo).await;
    revealed_discussion(&f.pool, bo.id, f.ana.id).await;
    revealed_discussion(&f.pool, f.ana.id, bo.id).await;
    // A grant for someone else's thread is untouched.
    revealed_discussion(&f.pool, cy.id, f.ana.id).await;
    assert_eq!(grants().await, 3);

    // Withdrawing nothing revokes nothing; unfollowing revokes both sides of the pair, so
    // following again does not reopen a thread revealed before.
    assert_eq!(f.unfollow(&f.ana, cy.id).await, StatusCode::NO_CONTENT);
    assert_eq!(grants().await, 3);
    assert_eq!(f.unfollow(&f.ana, bo.id).await, StatusCode::NO_CONTENT);
    assert_eq!(grants().await, 1);
    f.follow(&f.ana, bo.id).await;
    assert_eq!(grants().await, 1);

    // Removing a follower ends mutuality too.
    revealed_discussion(&f.pool, bo.id, f.ana.id).await;
    let (status, _) = f
        .write(
            &bo,
            Method::DELETE,
            &format!("/me/followers/{}", f.ana.id),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(grants().await, 1);
}

#[sqlx::test]
async fn self_unknown_blocked_and_handleless_follows_are_rejected_without_writes(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let cy = f.member("cy_c", "public").await;
    block(&f.pool, bo.id, f.ana.id).await;
    block(&f.pool, f.ana.id, cy.id).await;

    let (status, body) = f.follow(&f.ana, f.ana.id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["id"].is_string());
    for target in [bo.id, cy.id, Uuid::new_v4()] {
        assert_eq!(f.follow(&f.ana, target).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            f.get(&f.ana, &format!("/people/{target}")).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(f.library(&f.ana, target).await, StatusCode::NOT_FOUND);
    }
    // Blocks conceal in both directions.
    assert_eq!(f.follow(&bo, f.ana.id).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        f.get(&cy, &format!("/people/{}", f.ana.id)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.get(&f.ana, "/people/not-a-uuid").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.follow(&f.ana, Uuid::nil()).await.0, StatusCode::NOT_FOUND);

    // Disabled, unverified and handleless people are not followable.
    let gone = f.member("gone_d", "public").await;
    disable(&f.pool, gone.id).await;
    let pending = f.member("pending_p", "public").await;
    sqlx::query("UPDATE users SET verified_at = NULL WHERE id = $1")
        .bind(pending.id)
        .execute(&f.pool)
        .await
        .unwrap();
    let nameless = f.member("nameless", "public").await;
    sqlx::query("UPDATE users SET handle = NULL WHERE id = $1")
        .bind(nameless.id)
        .execute(&f.pool)
        .await
        .unwrap();
    for target in [gone.id, pending.id, nameless.id] {
        assert_eq!(f.follow(&f.ana, target).await.0, StatusCode::NOT_FOUND);
    }

    // The caller needs a handle for every social write.
    let (status, body) = f.follow(&nameless, f.ana.id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["handle"].is_string());
    assert_eq!(
        f.decide(&nameless, f.ana.id, "approve").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(count(&f.pool, "SELECT count(*) FROM follows").await, 0);
    assert_eq!(f.notifications().await, (0, 0));

    let (status, body) = f
        .write(
            &bo,
            Method::POST,
            &format!("/me/follow-requests/{}", f.ana.id),
            json!({"decision": "maybe"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["decision"].is_string());
    let (status, _) = f
        .write(
            &f.ana,
            Method::POST,
            &format!("/people/{}/follow", cy.id),
            json!({"x": 1}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown keys are rejected"
    );

    // Social APIs need a verified email.
    let unverified = TestUser::create(&f.test.security, false, "member").await;
    let (status, headers, body) =
        send(&f.app, Call::get("/people?q=bo").signed(&unverified.signed)).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
}

#[sqlx::test]
async fn a_request_from_someone_now_blocked_or_disabled_cannot_be_approved(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    let cy = f.member("cy_c", "private").await;
    f.follow(&bo, f.ana.id).await;
    f.follow(&cy, f.ana.id).await;
    block(&f.pool, bo.id, f.ana.id).await;
    disable(&f.pool, cy.id).await;
    assert_eq!(
        f.get(&f.ana, "/me/follow-requests").await.1["items"],
        json!([])
    );
    for requester in [bo.id, cy.id] {
        assert_eq!(
            f.decide(&f.ana, requester, "approve").await,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        count(
            &f.pool,
            "SELECT count(*) FROM follows WHERE state = 'approved'"
        )
        .await,
        0
    );
}

#[sqlx::test]
async fn search_lists_cards_only_and_never_matches_email_or_concealed_people(pool: PgPool) {
    let f = fixture(pool).await;
    let names = ["orchard_a", "orchard_b", "orchard_c"];
    let mut people = Vec::new();
    for handle in names {
        people.push(f.member(handle, "private").await);
    }
    save_show(&f.pool, people[0].id, "Secret Garden").await;
    let blocked = f.member("orchard_blocked", "public").await;
    block(&f.pool, blocked.id, f.ana.id).await;
    let gone = f.member("orchard_gone", "public").await;
    disable(&f.pool, gone.id).await;
    // Display names match too.
    let named = f.member("zed", "public").await;
    profile(&f.pool, named.id, "zed", "Orchard Keeper", "public").await;
    // Ana herself is never a result.
    profile(&f.pool, f.ana.id, "orchard_ana", "Ana", "private").await;

    let mut seen = Vec::new();
    let mut path = "/people?q=%20Orchard%20&limit=2".to_owned();
    loop {
        let (status, body) = f.get(&f.ana, &path).await;
        assert_eq!(status, StatusCode::OK);
        for item in body["items"].as_array().unwrap() {
            let mut keys: Vec<&str> = item
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                [
                    "displayName",
                    "handle",
                    "id",
                    "initials",
                    "relationship",
                    "visibility"
                ],
                "cards only: no library, progress or email"
            );
            seen.push(item["handle"].as_str().unwrap().to_owned());
        }
        assert!(!body.to_string().contains("Secret Garden"));
        assert!(!body.to_string().contains("example.test"));
        match body["nextCursor"].as_str() {
            Some(cursor) => path = format!("/people?q=orchard&limit=2&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(seen, ["orchard_a", "orchard_b", "orchard_c", "zed"]);

    // Email addresses are never searched.
    let (_, body) = f.get(&f.ana, "/people?q=example.test").await;
    assert_eq!(body["items"], json!([]));
    // LIKE wildcards are literal.
    let (_, body) = f.get(&f.ana, "/people?q=%25_").await;
    assert_eq!(body["items"], json!([]));

    for (query, code) in [
        ("q=a", 422),
        ("q=%20%20a%20", 422),
        ("", 422),
        ("q=orchard&limit=51", 422),
    ] {
        let (status, body) = f.get(&f.ana, &format!("/people?{query}")).await;
        assert_eq!(status.as_u16(), code, "{query}: {body}");
    }
    let long = "a".repeat(101);
    assert_eq!(
        f.get(&f.ana, &format!("/people?q={long}")).await.0.as_u16(),
        422
    );
    let (status, body) = f.get(&f.ana, "/people?q=orchard&cursor=not-a-cursor").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[sqlx::test]
async fn follower_request_and_library_lists_page_by_cursor(pool: PgPool) {
    let f = fixture(pool).await;
    let mut requesters = Vec::new();
    for handle in ["fan_one", "fan_two", "fan_three"] {
        let fan = f.member(handle, "private").await;
        f.follow(&fan, f.ana.id).await;
        requesters.push(fan.id);
    }
    // Newest first.
    requesters.reverse();
    for path in ["/me/follow-requests", "/me/followers"] {
        let mut seen = Vec::new();
        let mut next = format!("{path}?limit=2");
        loop {
            let (status, body) = f.get(&f.ana, &next).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            for item in body["items"].as_array().unwrap() {
                seen.push(item["id"].as_str().unwrap().parse::<Uuid>().unwrap());
            }
            match body["nextCursor"].as_str() {
                Some(cursor) => next = format!("{path}?limit=2&cursor={cursor}"),
                None => break,
            }
        }
        assert_eq!(seen, requesters, "{path}");
        let (status, _) = f.get(&f.ana, &format!("{path}?cursor=bad")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        for id in requesters.clone() {
            f.decide(&f.ana, id, "approve").await;
        }
    }

    // Bo's library pages saved_at,id descending.
    let bo = f.member("bo_i", "public").await;
    let titles = ["First", "Second", "Third"];
    for title in titles {
        save_show(&f.pool, bo.id, title).await;
    }
    let mut seen = Vec::new();
    let mut next = format!("/people/{}/library?limit=2", bo.id);
    loop {
        let (_, body) = f.get(&f.ana, &next).await;
        for item in body["items"].as_array().unwrap() {
            seen.push(item["title"].as_str().unwrap().to_owned());
        }
        match body["nextCursor"].as_str() {
            Some(cursor) => next = format!("/people/{}/library?limit=2&cursor={cursor}", bo.id),
            None => break,
        }
    }
    assert_eq!(seen, ["Third", "Second", "First"]);
    let (status, _) = f
        .get(&f.ana, &format!("/people/{}/library?cursor=bad", bo.id))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn concurrent_follows_create_one_edge_one_notification_and_one_event(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let app = f.app.clone();
            let call_cookie = f.ana.signed.cookie.clone();
            let csrf = f.ana.signed.csrf.clone();
            let path = format!("/people/{}/follow", bo.id);
            tokio::spawn(async move {
                let mut call = Call::post(&path, "{}");
                call.cookie = Some(&call_cookie);
                call.csrf = Some(&csrf);
                send(&app, call).await
            })
        })
        .collect();
    for task in tasks {
        let (status, _, body) = task.await.unwrap();
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({"state": "pending"}))
        );
    }
    assert_eq!(count(&f.pool, "SELECT count(*) FROM follows").await, 1);
    assert_eq!(f.notifications().await, (1, 1));
}

#[sqlx::test]
async fn racing_approval_and_withdrawal_leave_no_edge_or_notification(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    for _ in 0..5 {
        f.follow(&bo, f.ana.id).await;
        let (approved, withdrawn) = tokio::join!(
            f.decide(&f.ana, bo.id, "approve"),
            f.unfollow(&bo, f.ana.id)
        );
        assert!(
            [StatusCode::NO_CONTENT, StatusCode::NOT_FOUND].contains(&approved),
            "{approved}"
        );
        assert_eq!(withdrawn, StatusCode::NO_CONTENT);
        assert_eq!(count(&f.pool, "SELECT count(*) FROM follows").await, 0);
        assert_eq!(f.notifications().await.0, 0);
        assert_eq!(f.library(&bo, f.ana.id).await, StatusCode::NOT_FOUND);
    }
}
