//! R15 library API evidence (C05) against migrated PostgreSQL: owner isolation, expected
//! revisions, idempotency, action provenance, filter counts, search, keyset pages and history
//! retention across removal. Catalog rows are seeded directly; no provider is involved.

use std::sync::atomic::{AtomicI64, Ordering};

use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use chrono::Utc;
use scenecask_api::modules::library::{self, dto::Status, repository};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{Call, TestApp, TestUser, assert_error, count, send};

static PROVIDER_IDS: AtomicI64 = AtomicI64::new(1);

/// A complete show with regular and special episodes. `None` air dates are undated. Episode
/// titles and stills use the protected sentinels that `assert_error` and the tests look for.
async fn seed_show(
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

async fn watch(pool: &PgPool, user: Uuid, episode: Uuid) {
    sqlx::query("INSERT INTO episode_progress (user_id, episode_id, watched, revision) VALUES ($1, $2, true, 1)")
        .bind(user)
        .bind(episode)
        .execute(pool)
        .await
        .unwrap();
}

struct Fixture {
    app: Router,
    pool: PgPool,
    ana: TestUser,
}

async fn fixture(pool: PgPool) -> Fixture {
    let test = TestApp::new(&pool, |_| {});
    let ana = TestUser::create(&test.security, true, "member").await;
    Fixture {
        app: test.routes(library::routes(test.security.clone())),
        pool,
        ana,
    }
}

impl Fixture {
    async fn write(
        &self,
        user: &TestUser,
        method: Method,
        show: Uuid,
        key: &str,
        body: Value,
    ) -> (StatusCode, HeaderMap, Value) {
        let path = format!("/library/{show}");
        let body = body.to_string();
        send(
            &self.app,
            Call::write(method, &path, &body)
                .idempotent(key)
                .signed(&user.signed),
        )
        .await
    }

    /// Ana's write with a fresh idempotency key.
    async fn mutate(
        &self,
        method: Method,
        show: Uuid,
        body: Value,
    ) -> (StatusCode, HeaderMap, Value) {
        let key = Uuid::new_v4().to_string();
        self.write(&self.ana, method, show, &key, body).await
    }

    async fn put(&self, show: Uuid, body: Value) -> (StatusCode, HeaderMap, Value) {
        self.mutate(Method::PUT, show, body).await
    }

    async fn patch(&self, show: Uuid, body: Value) -> (StatusCode, HeaderMap, Value) {
        self.mutate(Method::PATCH, show, body).await
    }

    async fn list(&self, user: &TestUser, query: &str) -> (StatusCode, HeaderMap, Value) {
        let path = format!("/library{query}");
        send(&self.app, Call::get(&path).signed(&user.signed)).await
    }

    /// Every row a library write may touch, to prove rejected writes change nothing.
    async fn state(&self) -> Value {
        let rows: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object(
                 'entries', (SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY show_id), '[]') FROM library_entries e),
                 'actions', (SELECT count(*) FROM mutation_actions),
                 'changes', (SELECT count(*) FROM mutation_changes),
                 'keys', (SELECT count(*) FROM idempotency_records),
                 'tracking', (SELECT count(*) FROM tracking_show_state))",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap();
        rows
    }
}

fn released(season: i32, number: i32) -> (i32, i32, Option<&'static str>) {
    (season, number, Some("2024-01-01"))
}

#[sqlx::test]
async fn saving_records_one_action_and_returns_the_mutation_result(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    let (status, headers, body) = f
        .put(
            show,
            json!({"saved": true, "status": "plan_to_watch", "expectedRevision": 0}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(body["changed"], 1);
    assert_eq!(body["trackingRevision"], 0);
    assert_eq!(body["episodes"], json!([]));
    let action: Uuid = body["actionId"].as_str().unwrap().parse().unwrap();
    let undo_until =
        chrono::DateTime::parse_from_rfc3339(body["undoUntil"].as_str().unwrap()).unwrap();
    let window = undo_until.with_timezone(&Utc) - Utc::now();
    assert!(window > chrono::Duration::minutes(9) && window <= chrono::Duration::minutes(10));
    assert_eq!(
        body["library"],
        json!({
            "showId": show, "title": "Hollow Orchard", "year": null,
            "posterUrl": "https://image.tmdb.org/t/p/w500/poster.jpg",
            "saved": true, "status": "plan_to_watch", "revision": 1,
            "progress": {"watched": 0, "total": 1, "percent": 0, "state": "in_progress",
                         "nextEpisode": body["library"]["progress"]["nextEpisode"].clone(),
                         "outOfOrder": false, "releaseInfoIncomplete": false},
        })
    );
    let kind: String = sqlx::query_scalar("SELECT kind FROM mutation_actions WHERE id = $1")
        .bind(action)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(kind, "library_save");
    let changes: Vec<(String, String, Value, i64)> = sqlx::query_as(
        "SELECT entity, field, before_value, after_revision FROM mutation_changes
         WHERE action_id = $1 ORDER BY field",
    )
    .bind(action)
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        changes,
        [
            ("library_entry".into(), "saved".into(), json!(false), 1),
            ("library_entry".into(), "saved_at".into(), Value::Null, 1),
        ],
        "status kept its plan_to_watch default, so only saved and saved_at changed"
    );
}

#[sqlx::test]
async fn same_key_replays_and_changed_body_conflicts_without_writing(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;
    let key = Uuid::new_v4().to_string();
    let request = json!({"saved": true, "expectedRevision": 0});
    let (_, _, first) = f
        .write(&f.ana, Method::PUT, show, &key, request.clone())
        .await;
    let after_first = f.state().await;

    // Same key and body (any key order) returns the stored outcome, not a revision conflict.
    let reordered = json!({"expectedRevision": 0, "saved": true});
    let (status, _, replay) = f.write(&f.ana, Method::PUT, show, &key, reordered).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, first);
    assert_eq!(f.state().await, after_first);

    for (method, body) in [
        (Method::PUT, json!({"saved": false, "expectedRevision": 1})),
        (
            Method::PATCH,
            json!({"status": "watching", "expectedRevision": 1}),
        ),
    ] {
        let (status, headers, body) = f.write(&f.ana, method, show, &key, body).await;
        assert_error(status, &headers, &body, 409, "REVISION_CONFLICT");
    }
    assert_eq!(f.state().await, after_first);

    // Keys are per user: another user's identical key is a fresh request.
    let test = TestApp::new(&f.pool, |_| {});
    let ben = TestUser::create(&test.security, true, "member").await;
    let (status, _, body) = f.write(&ben, Method::PUT, show, &key, request).await;
    assert_eq!((status, &body["changed"]), (StatusCode::OK, &json!(1)));
}

#[sqlx::test]
async fn idempotency_key_is_required(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;
    let path = format!("/library/{show}");
    let body = json!({"saved": true, "expectedRevision": 0}).to_string();
    for key in [None, Some("not-a-uuid")] {
        let mut call = Call::write(Method::PUT, &path, &body).signed(&f.ana.signed);
        call.idempotency_key = key;
        let (status, headers, body) = send(&f.app, call).await;
        assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    }
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM library_entries").await,
        0
    );
}

#[sqlx::test]
async fn stale_revision_and_manual_completed_change_nothing(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "ended", &[released(1, 1)]).await;
    f.put(show, json!({"saved": true, "expectedRevision": 0}))
        .await;
    let before = f.state().await;

    for (method, body) in [
        (Method::PUT, json!({"saved": false, "expectedRevision": 0})),
        (
            Method::PUT,
            json!({"saved": true, "status": "watching", "expectedRevision": 2}),
        ),
        (
            Method::PATCH,
            json!({"status": "dropped", "expectedRevision": 0}),
        ),
    ] {
        let (status, headers, body) = f.mutate(method, show, body).await;
        assert_error(status, &headers, &body, 409, "REVISION_CONFLICT");
    }
    for (method, body) in [
        (
            Method::PUT,
            json!({"saved": true, "status": "completed", "expectedRevision": 1}),
        ),
        (
            Method::PATCH,
            json!({"status": "completed", "expectedRevision": 1}),
        ),
        (
            Method::PATCH,
            json!({"status": "caught_up", "expectedRevision": 1}),
        ),
    ] {
        let (status, headers, body) = f.mutate(method, show, body).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
        assert!(body["error"]["fields"]["status"].is_string());
    }
    for body in [
        json!({"saved": false, "status": "completed", "expectedRevision": 1}),
        json!({"saved": true, "expectedRevision": -1}),
        json!({"saved": true, "expectedRevision": 1, "role": "operator"}),
        json!({"expectedRevision": 1}),
    ] {
        let (status, headers, body) = f.put(show, body).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    }
    assert_eq!(f.state().await, before);
}

#[sqlx::test]
async fn removed_then_readded_show_restores_progress_and_status(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, episodes) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), released(1, 2), released(1, 3)],
    )
    .await;
    let (_, _, saved) = f
        .put(
            show,
            json!({"saved": true, "status": "on_hold", "expectedRevision": 0}),
        )
        .await;
    let saved_at: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT saved_at FROM library_entries")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    watch(&f.pool, f.ana.id, episodes[0]).await;
    watch(&f.pool, f.ana.id, episodes[2]).await;

    // Removal ignores a submitted status and keeps the manual one.
    let (status, _, removed) = f
        .put(
            show,
            json!({"saved": false, "status": "watching", "expectedRevision": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["changed"], 1);
    assert_eq!(removed["library"]["saved"], false);
    assert_eq!(removed["library"]["status"], "on_hold");
    assert_eq!(removed["library"]["revision"], 2);
    assert_eq!(removed["library"]["progress"]["watched"], 2);
    let (_, _, page) = f.list(&f.ana, "").await;
    assert_eq!(page["items"], json!([]));
    assert_eq!(page["counts"]["all"], 0);
    assert_eq!(
        count(
            &f.pool,
            "SELECT count(*) FROM episode_progress WHERE watched"
        )
        .await,
        2
    );

    // Re-adding without a status restores the manual status and all history.
    let (_, _, readded) = f
        .put(show, json!({"saved": true, "expectedRevision": 2}))
        .await;
    assert_eq!(readded["changed"], 1);
    let item = &readded["library"];
    assert_eq!(
        (&item["saved"], &item["status"], &item["revision"]),
        (&json!(true), &json!("on_hold"), &json!(3))
    );
    assert_eq!(item["progress"], removed["library"]["progress"]);
    assert_eq!(item["progress"]["nextEpisode"]["id"], json!(episodes[1]));
    assert_eq!(item["progress"]["outOfOrder"], true);
    assert_ne!(saved["actionId"], readded["actionId"]);
    let resaved_at: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT saved_at FROM library_entries")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert!(
        resaved_at >= saved_at,
        "re-adding moves the show to the top of the library"
    );
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM mutation_actions ORDER BY created_at, kind")
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&"library_remove".to_owned()));
}

#[sqlx::test]
async fn status_changes_need_a_saved_entry_and_unchanged_writes_are_no_ops(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;
    let (other, _) = seed_show(&f.pool, "Quiet Harbor", "returning", &[]).await;

    // Missing, unknown and non-UUID shows are 404 for PATCH; unknown shows are 404 for PUT.
    for target in [
        show.to_string(),
        Uuid::new_v4().to_string(),
        "not-a-show".into(),
    ] {
        let path = format!("/library/{target}");
        let body = json!({"status": "watching", "expectedRevision": 0}).to_string();
        let key = Uuid::new_v4().to_string();
        let (status, headers, body) = send(
            &f.app,
            Call::write(Method::PATCH, &path, &body)
                .idempotent(&key)
                .signed(&f.ana.signed),
        )
        .await;
        assert_error(status, &headers, &body, 404, "NOT_FOUND");
    }
    let (status, headers, body) = f
        .put(
            Uuid::new_v4(),
            json!({"saved": true, "expectedRevision": 0}),
        )
        .await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");

    // Removing a show that was never saved is a no-op without a row or action.
    let (status, _, body) = f
        .put(other, json!({"saved": false, "expectedRevision": 0}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&body["changed"], &body["actionId"], &body["undoUntil"]),
        (&json!(0), &Value::Null, &Value::Null)
    );
    assert_eq!(body["library"]["revision"], 0);
    assert_eq!(body["library"]["saved"], false);

    f.put(show, json!({"saved": true, "expectedRevision": 0}))
        .await;
    let (status, _, body) = f
        .patch(show, json!({"status": "watching", "expectedRevision": 1}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["changed"], 1);
    assert_eq!(body["library"]["status"], "watching");
    assert_eq!(body["library"]["revision"], 2);

    let actions = count(&f.pool, "SELECT count(*) FROM mutation_actions").await;
    for (method, body) in [
        (
            Method::PATCH,
            json!({"status": "watching", "expectedRevision": 2}),
        ),
        (Method::PUT, json!({"saved": true, "expectedRevision": 2})),
        (
            Method::PUT,
            json!({"saved": true, "status": "watching", "expectedRevision": 2}),
        ),
    ] {
        let (status, _, body) = f.mutate(method, show, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            (&body["changed"], &body["actionId"]),
            (&json!(0), &Value::Null)
        );
        assert_eq!(body["library"]["revision"], 2);
    }
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM mutation_actions").await,
        actions
    );

    // Removing keeps the status; PATCH on a removed entry is 404.
    f.put(show, json!({"saved": false, "expectedRevision": 2}))
        .await;
    let (status, headers, body) = f
        .patch(show, json!({"status": "dropped", "expectedRevision": 3}))
        .await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
}

#[sqlx::test]
async fn library_is_owner_isolated(pool: PgPool) {
    let f = fixture(pool).await;
    let test = TestApp::new(&f.pool, |_| {});
    let ben = TestUser::create(&test.security, true, "member").await;
    let (show, episodes) =
        seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    f.put(
        show,
        json!({"saved": true, "status": "watching", "expectedRevision": 0}),
    )
    .await;
    watch(&f.pool, f.ana.id, episodes[0]).await;

    let (_, _, page) = f.list(&ben, "").await;
    assert_eq!(page["items"], json!([]));
    assert_eq!(page["counts"]["all"], 0);

    // Ben's write uses his own revision space and never touches Ana's entry or progress.
    let key = Uuid::new_v4().to_string();
    let (status, headers, body) = f
        .write(
            &ben,
            Method::PATCH,
            show,
            &key,
            json!({"status": "dropped", "expectedRevision": 1}),
        )
        .await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let (status, _, body) = f
        .write(
            &ben,
            Method::PUT,
            show,
            &Uuid::new_v4().to_string(),
            json!({"saved": true, "expectedRevision": 0}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["library"]["status"], "plan_to_watch");
    assert_eq!(body["library"]["progress"]["watched"], 0);

    let (_, _, page) = f.list(&f.ana, "").await;
    assert_eq!(page["items"][0]["status"], "watching");
    assert_eq!(page["items"][0]["revision"], 1);
    assert_eq!(page["items"][0]["progress"]["watched"], 1);
}

#[sqlx::test]
async fn list_filters_counts_search_and_pages(pool: PgPool) {
    let f = fixture(pool).await;
    let mut shows = Vec::new();
    for (title, status) in [
        ("Hollow Orchard", "watching"),
        ("Hollow Bay", "plan_to_watch"),
        ("Quiet 100% Harbor", "watching"),
        ("Dust Lines", "dropped"),
        ("Removed Show", "on_hold"),
    ] {
        let (show, _) = seed_show(&f.pool, title, "returning", &[]).await;
        f.put(
            show,
            json!({"saved": true, "status": status, "expectedRevision": 0}),
        )
        .await;
        shows.push(show);
    }
    f.put(shows[4], json!({"saved": false, "expectedRevision": 1}))
        .await;
    // Distinct, known saved_at values: later shows are newer.
    for (index, show) in shows.iter().enumerate() {
        sqlx::query("UPDATE library_entries SET saved_at = now() - make_interval(mins => $2), revision = revision + 1 WHERE show_id = $1")
            .bind(show)
            .bind(10 - index as i32)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    let all_counts =
        json!({"all": 4, "watching": 2, "plan_to_watch": 1, "on_hold": 0, "dropped": 1});
    let titles = |page: &Value| -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["title"].as_str().unwrap().to_owned())
            .collect()
    };

    let (status, headers, page) = f.list(&f.ana, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(page["counts"], all_counts);
    assert_eq!(
        titles(&page),
        [
            "Dust Lines",
            "Quiet 100% Harbor",
            "Hollow Bay",
            "Hollow Orchard"
        ]
    );
    assert_eq!(page["nextCursor"], Value::Null);

    for (query, expected) in [
        (
            "?status=watching",
            vec!["Quiet 100% Harbor", "Hollow Orchard"],
        ),
        ("?q=%20hOLLOW%20", vec!["Hollow Bay", "Hollow Orchard"]),
        ("?q=hollow&status=plan_to_watch", vec!["Hollow Bay"]),
        ("?q=100%25", vec!["Quiet 100% Harbor"]),
        ("?q=%25", vec!["Quiet 100% Harbor"]),
        ("?q=_", vec![]),
        ("?q=removed", vec![]),
        ("?status=on_hold", vec![]),
        (
            "?q=",
            vec![
                "Dust Lines",
                "Quiet 100% Harbor",
                "Hollow Bay",
                "Hollow Orchard",
            ],
        ),
    ] {
        let (status, _, page) = f.list(&f.ana, query).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        assert_eq!(titles(&page), expected, "{query}");
        assert_eq!(page["counts"], all_counts, "counts ignore filters: {query}");
    }

    // Keyset pages walk every saved show exactly once.
    let mut seen = Vec::new();
    let mut query = "?limit=1".to_owned();
    loop {
        let (status, _, page) = f.list(&f.ana, &query).await;
        assert_eq!(status, StatusCode::OK);
        seen.extend(titles(&page));
        match page["nextCursor"].as_str() {
            Some(cursor) => query = format!("?limit=1&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(
        seen,
        [
            "Dust Lines",
            "Quiet 100% Harbor",
            "Hollow Bay",
            "Hollow Orchard"
        ]
    );
    let (_, _, page) = f.list(&f.ana, "?status=watching&limit=1").await;
    let cursor = page["nextCursor"].as_str().unwrap().to_owned();
    let (_, _, page) = f
        .list(&f.ana, &format!("?status=watching&limit=1&cursor={cursor}"))
        .await;
    assert_eq!(titles(&page), ["Hollow Orchard"]);

    for (query, code, status) in [
        ("?cursor=bogus", "VALIDATION_ERROR", 400),
        ("?status=completed", "VALIDATION_ERROR", 422),
        ("?limit=0", "VALIDATION_ERROR", 422),
        ("?limit=51", "VALIDATION_ERROR", 422),
        ("?limit=ten", "VALIDATION_ERROR", 422),
    ] {
        let (actual, headers, body) = f.list(&f.ana, query).await;
        assert_error(actual, &headers, &body, status, code);
    }
    let long = format!("?q={}", "a".repeat(101));
    let (status, headers, body) = f.list(&f.ana, &long).await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert!(body["error"]["fields"]["q"].is_string());
}

#[sqlx::test]
async fn library_items_carry_progress_without_protected_episode_data(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, episodes) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "ended",
        &[
            (0, 1, Some("2024-01-01")),
            released(1, 1),
            released(1, 2),
            (1, 3, Some("2999-01-01")),
            (1, 4, None),
        ],
    )
    .await;
    watch(&f.pool, f.ana.id, episodes[0]).await;
    watch(&f.pool, f.ana.id, episodes[2]).await;
    watch(&f.pool, f.ana.id, episodes[3]).await;
    f.put(
        show,
        json!({"saved": true, "status": "watching", "expectedRevision": 0}),
    )
    .await;
    let (_, _, page) = f.list(&f.ana, "").await;
    assert_eq!(
        page["items"][0]["progress"],
        json!({"watched": 2, "total": 4, "percent": 50, "state": "in_progress",
               "nextEpisode": {"id": episodes[1], "season": 1, "number": 1},
               "outOfOrder": true, "releaseInfoIncomplete": true})
    );
    let text = page.to_string();
    for protected in ["Protected", "protected-episode"] {
        assert!(!text.contains(protected), "library leaked {protected}");
    }
}

#[sqlx::test]
async fn home_source_uses_saved_watching_but_library_empty_counts_every_saved_show(pool: PgPool) {
    let f = fixture(pool).await;
    let (on_hold, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;
    let mut conn = f.pool.acquire().await.unwrap();
    let (items, any_saved) = repository::saved_watching(&mut conn, f.ana.id, Utc::now())
        .await
        .unwrap();
    assert!(items.is_empty() && !any_saved);

    f.put(
        on_hold,
        json!({"saved": true, "status": "on_hold", "expectedRevision": 0}),
    )
    .await;
    let (items, any_saved) = repository::saved_watching(&mut conn, f.ana.id, Utc::now())
        .await
        .unwrap();
    assert!(items.is_empty());
    assert!(any_saved, "an on-hold show means the library is not empty");

    let (watching, _) = seed_show(&f.pool, "Quiet Harbor", "returning", &[]).await;
    f.put(
        watching,
        json!({"saved": true, "status": "watching", "expectedRevision": 0}),
    )
    .await;
    let (removed, _) = seed_show(&f.pool, "Dust Lines", "returning", &[]).await;
    f.put(
        removed,
        json!({"saved": true, "status": "watching", "expectedRevision": 0}),
    )
    .await;
    f.put(removed, json!({"saved": false, "expectedRevision": 1}))
        .await;
    let (items, _) = repository::saved_watching(&mut conn, f.ana.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].show_id, items[0].status),
        (watching, Status::Watching)
    );
}

#[sqlx::test]
async fn library_requires_a_verified_session_and_csrf(pool: PgPool) {
    let f = fixture(pool).await;
    let test = TestApp::new(&f.pool, |_| {});
    let pending = TestUser::create(&test.security, false, "member").await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;

    let (status, headers, body) = send(&f.app, Call::get("/library")).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    let (status, headers, body) = f.list(&pending, "").await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    let key = Uuid::new_v4().to_string();
    let (status, headers, body) = f
        .write(
            &pending,
            Method::PUT,
            show,
            &key,
            json!({"saved": true, "expectedRevision": 0}),
        )
        .await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");

    let path = format!("/library/{show}");
    let request = json!({"saved": true, "expectedRevision": 0}).to_string();
    let mut call = Call::write(Method::PUT, &path, &request)
        .idempotent(&key)
        .signed(&f.ana.signed);
    call.csrf = None;
    let (status, headers, body) = send(&f.app, call).await;
    assert_error(status, &headers, &body, 403, "CSRF_FAILED");
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM library_entries").await,
        0
    );
}

#[sqlx::test]
async fn concurrent_first_saves_produce_one_entry(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, _) = seed_show(&f.pool, "Hollow Orchard", "returning", &[]).await;
    let request = || f.put(show, json!({"saved": true, "expectedRevision": 0}));
    let ((a, _, _), (b, _, _)) = tokio::join!(request(), request());
    let mut statuses = [a.as_u16(), b.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM library_entries").await,
        1
    );
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM mutation_actions").await,
        1
    );
}
