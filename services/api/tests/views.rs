//! R18 viewer-filtered projection evidence (C05 Show/Episode/Home, C07 reveals, C09 cases 1, 7
//! and 9) against migrated PostgreSQL: locked episode JSON carries no protected keys, watched and
//! revealed episodes unlock only for their viewer and session, Hide again and logout relock,
//! discussion reveals pass the privacy gate first, and Home distinguishes an on-hold-only
//! library from an empty one.

use axum::http::{HeaderMap, Method, StatusCode, header};
use scenecask_api::modules::{auth::session, catalog::views, library, policy, tracking};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, TestUser, assert_csrf_required, assert_error, count, count_session,
    member_fixture, released, seed_show, send, sign_in,
};

type Response = (StatusCode, HeaderMap, Value);

/// Every protected value `seed_show` stores, in any form a response could carry it.
const PROTECTED: [&str; 4] = [
    "Protected title",
    "Protected overview",
    "protected-episode",
    "stillUrl",
];

async fn fixture(pool: PgPool) -> MemberFixture {
    member_fixture(pool, |security| {
        views::routes(security.clone())
            .merge(tracking::home::routes(security.clone()))
            .merge(policy::routes(security.clone()))
            .merge(tracking::routes(security.clone()))
            .merge(library::routes(security.clone()))
            .merge(session::routes(security.clone()))
    })
    .await
}

impl MemberFixture {
    /// A complete returning show named Hollow Orchard.
    async fn seed(&self, episodes: &[(i32, i32, Option<&str>)]) -> (Uuid, Vec<Uuid>) {
        seed_show(&self.pool, "Hollow Orchard", "returning", episodes).await
    }

    async fn user(&self) -> TestUser {
        TestUser::create(&self.test.security, true, "member").await
    }

    async fn get_as(&self, user: &TestUser, path: &str) -> Response {
        let response = send(&self.app, Call::get(path).signed(&user.signed)).await;
        assert_eq!(response.1[header::CACHE_CONTROL], "private, no-store");
        response
    }

    async fn get(&self, path: &str) -> Value {
        let (status, _, body) = self.get_as(&self.ana, path).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn write(
        &self,
        user: &TestUser,
        method: Method,
        path: &str,
        key: Option<&str>,
        body: Value,
    ) -> Response {
        let body = body.to_string();
        let mut call = Call::write(method, path, &body).signed(&user.signed);
        call.idempotency_key = key;
        send(&self.app, call).await
    }

    async fn mark(&self, user: &TestUser, episode: Uuid, watched: bool, expected: i64) {
        let key = Uuid::new_v4().to_string();
        let path = format!("/progress/episodes/{episode}");
        let body = json!({"watched": watched, "expectedRevision": expected});
        let (status, _, body) = self.write(user, Method::PUT, &path, Some(&key), body).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn save(&self, show: Uuid, status: &str) {
        let key = Uuid::new_v4().to_string();
        let body = json!({"saved": true, "status": status, "expectedRevision": 0});
        let path = format!("/library/{show}");
        let (code, _, body) = self
            .write(&self.ana, Method::PUT, &path, Some(&key), body)
            .await;
        assert_eq!(code, StatusCode::OK, "{body}");
    }

    async fn reveal(&self, user: &TestUser, scope: &str, resource: Uuid) -> Response {
        let body = json!({"scope": scope, "resourceId": resource});
        self.write(user, Method::POST, "/reveals", None, body).await
    }

    async fn hide(&self, user: &TestUser, scope: &str, resource: Uuid) -> Response {
        let path = format!("/reveals/{scope}/{resource}");
        self.write(user, Method::DELETE, &path, None, json!({}))
            .await
    }
}

fn assert_locked(episode: &Value) {
    assert_eq!(episode["detailsAccess"], "locked", "{episode}");
    let mut keys: Vec<&str> = episode
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "detailsAccess",
            "id",
            "number",
            "release",
            "revision",
            "season",
            "showId",
            "watched"
        ]
    );
    assert_no_protected(episode);
}

fn assert_no_protected(value: &Value) {
    let text = value.to_string();
    for secret in PROTECTED
        .into_iter()
        .chain(["title\":\"Protected", "overview"])
    {
        assert!(!text.contains(secret), "leaked {secret:?} in {text}");
    }
}

fn assert_unlocked(episode: &Value, access: &str) {
    assert_eq!(episode["detailsAccess"], access, "{episode}");
    assert_eq!(
        episode["details"],
        json!({
            "title": "Protected title",
            "overview": "Protected overview",
            "stillUrl": "https://image.tmdb.org/t/p/w300/protected-episode.jpg",
        })
    );
}

/// Acceptance criterion 1 and C09 case 1: an unwatched episode has no details at all, in the
/// single and list projections, while a watched later episode unlocks only itself.
#[sqlx::test]
async fn unwatched_episodes_omit_protected_details(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = f
        .seed(&[released(1, 1), released(1, 2), released(1, 3)])
        .await;

    let e2 = f.get(&format!("/episodes/{}", eps[1])).await;
    assert_locked(&e2);
    assert_eq!(e2["showId"], show.to_string());
    assert_eq!(
        e2["release"],
        json!({"state": "released", "date": "2024-01-01", "timezone": "UTC", "estimated": true})
    );
    assert_eq!(
        (e2["watched"].clone(), e2["revision"].clone()),
        (json!(false), json!(0))
    );

    f.mark(&f.ana, eps[2], true, 0).await;
    let page = f.get(&format!("/shows/{show}/episodes")).await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_locked(&items[0]);
    assert_locked(&items[1]);
    assert_unlocked(&items[2], "watched");
    assert_eq!(items[2]["revision"], 1);

    // Another viewer's progress never unlocks this viewer's details.
    let bo = f.user().await;
    let (status, _, e3) = f.get_as(&bo, &format!("/episodes/{}", eps[2])).await;
    assert_eq!(status, StatusCode::OK);
    assert_locked(&e3);

    // Unmarking relocks.
    f.mark(&f.ana, eps[2], false, 1).await;
    assert_locked(&f.get(&format!("/episodes/{}", eps[2])).await);
}

/// Acceptance criterion 2 and C09 case 9: a reveal unlocks one episode for one session; Hide
/// again and logout relock it, and no other session or viewer can use it.
#[sqlx::test]
async fn reveal_unlocks_one_episode_for_one_session_until_hidden(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, eps) = f.seed(&[released(1, 1), (1, 2, Some("2999-01-01"))]).await;
    let path = format!("/episodes/{}", eps[1]);

    let (status, _, body) = f.reveal(&f.ana, "episode_details", eps[1]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["scope"], "episode_details");
    assert_eq!(body["resourceId"], eps[1].to_string());
    let expires: chrono::DateTime<chrono::Utc> =
        body["expiresAt"].as_str().unwrap().parse().unwrap();
    let remaining = expires - chrono::Utc::now();
    assert!(remaining <= chrono::Duration::hours(12) && remaining > chrono::Duration::hours(11));

    let revealed = f.get(&path).await;
    assert_unlocked(&revealed, "revealed");
    // A reveal marks nothing and keeps the future label.
    assert_eq!(revealed["watched"], false);
    assert_eq!(revealed["release"]["state"], "future");
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM episode_progress").await,
        0
    );
    assert_locked(&f.get(&format!("/episodes/{}", eps[0])).await);

    // Same user, another session; another user: both still locked.
    let second = TestUser {
        id: f.ana.id,
        signed: sign_in(&f.test.security, f.ana.id).await,
    };
    assert_locked(&f.get_as(&second, &path).await.2);
    let bo = f.user().await;
    assert_locked(&f.get_as(&bo, &path).await.2);

    // Repeating renews the same grant.
    assert_eq!(
        f.reveal(&f.ana, "episode_details", eps[1]).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        count_session(&f.pool, "reveal_grants", &f.ana.signed.hash).await,
        1
    );

    let (status, _, body) = f.hide(&f.ana, "episode_details", eps[1]).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_locked(&f.get(&path).await);
    // Hide again is idempotent.
    assert_eq!(
        f.hide(&f.ana, "episode_details", eps[1]).await.0,
        StatusCode::NO_CONTENT
    );

    // Logout revokes every grant of the session.
    assert_eq!(
        f.reveal(&f.ana, "episode_details", eps[1]).await.0,
        StatusCode::CREATED
    );
    let (status, _, _) = f
        .write(&f.ana, Method::POST, "/auth/logout", None, json!({}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count_session(&f.pool, "reveal_grants", &f.ana.signed.hash).await,
        0
    );
    let (status, headers, body) = f.get_as(&f.ana, &path).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
}

/// An expired grant and an expired session never unlock details.
#[sqlx::test]
async fn expired_grants_and_sessions_stay_locked(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, eps) = f.seed(&[released(1, 1)]).await;
    let path = format!("/episodes/{}", eps[0]);
    assert_eq!(
        f.reveal(&f.ana, "episode_details", eps[0]).await.0,
        StatusCode::CREATED
    );
    // The table only accepts future expiries, so let a one-millisecond grant lapse.
    sqlx::query("UPDATE reveal_grants SET expires_at = created_at + interval '1 millisecond'")
        .execute(&f.pool)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert_locked(&f.get(&path).await);

    // A grant never outlives its session's absolute expiry.
    sqlx::query("UPDATE sessions SET expires_at = now() + interval '1 hour' WHERE id_hash = $1")
        .bind(&f.ana.signed.hash)
        .execute(&f.pool)
        .await
        .unwrap();
    let (_, _, body) = f.reveal(&f.ana, "episode_details", eps[0]).await;
    let expires: chrono::DateTime<chrono::Utc> =
        body["expiresAt"].as_str().unwrap().parse().unwrap();
    assert!(expires <= chrono::Utc::now() + chrono::Duration::hours(1));

    // Idle expiry deletes the session and its grants on the next request.
    sqlx::query(
        "UPDATE sessions SET last_seen_at = now() - interval '7 days 1 second' WHERE id_hash = $1",
    )
    .bind(&f.ana.signed.hash)
    .execute(&f.pool)
    .await
    .unwrap();
    let (status, headers, body) = f.get_as(&f.ana, &path).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    assert_eq!(
        count_session(&f.pool, "reveal_grants", &f.ana.signed.hash).await,
        0
    );
}

async fn discussion(pool: &PgPool, host: Uuid, episode: Uuid) -> Uuid {
    sqlx::query_scalar("INSERT INTO discussions (host_id, episode_id) VALUES ($1, $2) RETURNING id")
        .bind(host)
        .bind(episode)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn follow(pool: &PgPool, follower: Uuid, followee: Uuid) {
    sqlx::query(
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ($1, $2, 'approved')",
    )
    .bind(follower)
    .bind(followee)
    .execute(pool)
    .await
    .unwrap();
}

/// C07/C09 case 7: discussion reveals check current privacy membership before any grant exists.
#[sqlx::test]
async fn discussion_reveals_require_current_membership(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, eps) = f.seed(&[released(1, 1)]).await;
    let host = f.user().await;
    let thread = discussion(&f.pool, host.id, eps[0]).await;
    let grants = || {
        count(
            &f.pool,
            "SELECT count(*) FROM reveal_grants WHERE scope = 'discussion'",
        )
    };

    // Unknown thread and one-way follow are concealed alike.
    for resource in [Uuid::new_v4(), thread] {
        let (status, headers, body) = f.reveal(&f.ana, "discussion", resource).await;
        assert_error(status, &headers, &body, 404, "NOT_FOUND");
    }
    follow(&f.pool, f.ana.id, host.id).await;
    let (status, headers, body) = f.reveal(&f.ana, "discussion", thread).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    assert_eq!(grants().await, 0);

    // The host and a mutual approved follow may reveal.
    assert_eq!(
        f.reveal(&host, "discussion", thread).await.0,
        StatusCode::CREATED
    );
    follow(&f.pool, host.id, f.ana.id).await;
    let (status, _, body) = f.reveal(&f.ana, "discussion", thread).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["scope"], "discussion");
    assert_eq!(grants().await, 2);

    // A block in either direction conceals the thread again.
    sqlx::query("INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)")
        .bind(host.id)
        .bind(f.ana.id)
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, headers, body) = f.reveal(&f.ana, "discussion", thread).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    // A discussion grant never unlocks episode details.
    assert_locked(&f.get(&format!("/episodes/{}", eps[0])).await);
}

#[sqlx::test]
async fn reveal_requests_are_validated(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, eps) = f.seed(&[released(1, 1)]).await;

    let (status, headers, body) = f.reveal(&f.ana, "episode_details", Uuid::new_v4()).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let (status, headers, body) = f.reveal(&f.ana, "everything", eps[0]).await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert!(body["error"]["fields"]["scope"].is_string());
    for invalid in [
        json!({"scope": "episode_details", "resourceId": "x"}),
        json!({"scope": "episode_details", "resourceId": eps[0], "extra": 1}),
        json!({"scope": "episode_details"}),
    ] {
        let (status, headers, body) = f
            .write(&f.ana, Method::POST, "/reveals", None, invalid)
            .await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    }
    for path in [
        format!("/reveals/everything/{}", eps[0]),
        "/reveals/episode_details/not-a-uuid".to_owned(),
    ] {
        let (status, headers, body) = f
            .write(&f.ana, Method::DELETE, &path, None, json!({}))
            .await;
        assert_error(status, &headers, &body, 404, "NOT_FOUND");
    }
    let body = json!({"scope": "episode_details", "resourceId": eps[0]}).to_string();
    assert_csrf_required(&f.app, &f.ana, Method::POST, "/reveals", &body).await;
    let path = format!("/reveals/episode_details/{}", eps[0]);
    assert_csrf_required(&f.app, &f.ana, Method::DELETE, &path, "{}").await;
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM reveal_grants").await,
        0
    );
}

/// Episode lists: season, number, id order; keyset pages; specials only on explicit season=0;
/// archived episodes leave the list.
#[sqlx::test]
async fn episode_lists_are_ordered_paginated_and_specials_explicit(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = f
        .seed(&[
            released(2, 1),
            released(1, 2),
            (0, 1, Some("2024-01-01")),
            released(1, 1),
            (1, 3, None),
            released(1, 4),
        ])
        .await;
    sqlx::query("UPDATE episodes SET archived_at = now() WHERE id = $1")
        .bind(eps[5])
        .execute(&f.pool)
        .await
        .unwrap();

    let codes = |page: &Value| -> Vec<(i64, i64)> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["season"].as_i64().unwrap(), e["number"].as_i64().unwrap()))
            .collect()
    };
    let all = f.get(&format!("/shows/{show}/episodes")).await;
    assert_eq!(codes(&all), [(1, 1), (1, 2), (1, 3), (2, 1)]);
    assert_eq!(all["nextCursor"], Value::Null);
    assert_eq!(all["items"][2]["release"]["state"], "unknown");
    assert_no_protected(&all);

    let first = f.get(&format!("/shows/{show}/episodes?limit=3")).await;
    assert_eq!(codes(&first), [(1, 1), (1, 2), (1, 3)]);
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = f
        .get(&format!("/shows/{show}/episodes?limit=3&cursor={cursor}"))
        .await;
    assert_eq!(codes(&second), [(2, 1)]);
    assert_eq!(second["nextCursor"], Value::Null);

    let specials = f.get(&format!("/shows/{show}/episodes?season=0")).await;
    assert_eq!(codes(&specials), [(0, 1)]);
    let season = f.get(&format!("/shows/{show}/episodes?season=1")).await;
    assert_eq!(codes(&season), [(1, 1), (1, 2), (1, 3)]);

    // An archived episode is still addressable directly.
    assert_locked(&f.get(&format!("/episodes/{}", eps[5])).await);

    for (query, status, code) in [
        ("season=-1", 422, "VALIDATION_ERROR"),
        ("season=x", 422, "VALIDATION_ERROR"),
        ("limit=0", 422, "VALIDATION_ERROR"),
        ("limit=51", 422, "VALIDATION_ERROR"),
        ("cursor=nope", 400, "VALIDATION_ERROR"),
    ] {
        let (actual, headers, body) = f
            .get_as(&f.ana, &format!("/shows/{show}/episodes?{query}"))
            .await;
        assert_error(actual, &headers, &body, status, code);
    }
    for path in [
        format!("/shows/{}/episodes", Uuid::new_v4()),
        "/shows/not-a-uuid/episodes".to_owned(),
        format!("/episodes/{}", Uuid::new_v4()),
        format!("/shows/{}", Uuid::new_v4()),
    ] {
        let (status, headers, body) = f.get_as(&f.ana, &path).await;
        assert_error(status, &headers, &body, 404, "NOT_FOUND");
    }
}

/// The Show DTO carries series-level data and the viewer's own library and tracking revision.
#[sqlx::test]
async fn show_dto_is_series_level_and_viewer_scoped(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = f
        .seed(&[released(1, 1), released(1, 2), (1, 3, None), (0, 1, None)])
        .await;
    sqlx::query(
        "UPDATE shows SET genres = '[\"Drama\"]', first_air_year = 2024,
                          fetched_at = now() - interval '3 days' WHERE id = $1",
    )
    .bind(show)
    .execute(&f.pool)
    .await
    .unwrap();

    let body = f.get(&format!("/shows/{show}")).await;
    assert_eq!(
        body,
        json!({
            "id": show,
            "title": "Hollow Orchard",
            "year": 2024,
            "genres": ["Drama"],
            "synopsis": "",
            "posterUrl": "https://image.tmdb.org/t/p/w500/poster.jpg",
            "status": "returning",
            "catalogRevision": body["catalogRevision"],
            "metadataStale": true,
            "releaseInfoIncomplete": true,
            "seasons": [{"number": 0, "count": 1}, {"number": 1, "count": 3}],
            "library": null,
            "trackingRevision": 0,
        })
    );

    f.mark(&f.ana, eps[1], true, 0).await;
    let body = f.get(&format!("/shows/{show}")).await;
    assert_eq!(body["trackingRevision"], 1);
    assert_eq!(body["library"]["status"], "watching");
    assert_eq!(body["library"]["progress"]["watched"], 1);
    assert_eq!(
        body["library"]["progress"]["nextEpisode"]["id"],
        eps[0].to_string()
    );
    assert_no_protected(&body);

    // Another viewer sees no library entry or tracking revision.
    let bo = f.user().await;
    let (_, _, other) = f.get_as(&bo, &format!("/shows/{show}")).await;
    assert_eq!(other["library"], Value::Null);
    assert_eq!(other["trackingRevision"], 0);
}

/// Acceptance criterion 3: Home sections come only from saved Watching shows, and an
/// on-hold-only library is not reported as empty.
#[sqlx::test]
async fn home_separates_choose_a_show_from_empty_library(pool: PgPool) {
    let f = fixture(pool).await;
    assert_eq!(
        f.get("/home").await,
        json!({"upNext": [], "caughtUp": [], "libraryEmpty": true})
    );

    let (paused, _) = seed_show(&f.pool, "Paused", "returning", &[released(1, 1)]).await;
    let (planned, _) = seed_show(&f.pool, "Planned", "returning", &[released(1, 1)]).await;
    f.save(paused, "on_hold").await;
    f.save(planned, "on_hold").await;
    assert_eq!(
        f.get("/home").await,
        json!({"upNext": [], "caughtUp": [], "libraryEmpty": false})
    );

    let (older, older_eps) = seed_show(
        &f.pool,
        "Older",
        "returning",
        &[released(1, 1), released(1, 2)],
    )
    .await;
    let (caught, caught_eps) = seed_show(&f.pool, "Caught", "returning", &[released(1, 1)]).await;
    let (newer, newer_eps) = seed_show(
        &f.pool,
        "Newer",
        "returning",
        &[released(1, 1), released(1, 2)],
    )
    .await;
    f.mark(&f.ana, older_eps[1], true, 0).await;
    f.mark(&f.ana, caught_eps[0], true, 0).await;
    f.mark(&f.ana, newer_eps[0], true, 0).await;

    let home = f.get("/home").await;
    let ids = |key: &str| -> Vec<String> {
        home[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["showId"].as_str().unwrap().to_owned())
            .collect()
    };
    // Featured first: saved_at,id descending.
    assert_eq!(ids("upNext"), [newer.to_string(), older.to_string()]);
    assert_eq!(ids("caughtUp"), [caught.to_string()]);
    assert_eq!(home["libraryEmpty"], false);
    assert_eq!(home["upNext"][1]["progress"]["outOfOrder"], true);
    assert_no_protected(&home);

    // Another viewer's library never leaks into Home.
    let bo = f.user().await;
    let (_, _, other) = f.get_as(&bo, "/home").await;
    assert_eq!(
        other,
        json!({"upNext": [], "caughtUp": [], "libraryEmpty": true})
    );
}

#[sqlx::test]
async fn projections_require_a_verified_session(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = f.seed(&[released(1, 1)]).await;
    let unverified = TestUser::create(&f.test.security, false, "member").await;
    for path in [
        format!("/shows/{show}"),
        format!("/shows/{show}/episodes"),
        format!("/episodes/{}", eps[0]),
        "/home".to_owned(),
    ] {
        let (status, headers, body) = send(&f.app, Call::get(&path)).await;
        assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
        let (status, headers, body) = f.get_as(&unverified, &path).await;
        assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    }
    let (status, headers, body) = f.reveal(&unverified, "episode_details", eps[0]).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
}
