//! R16 tracking write evidence (C05, C09 cases 1, 2 and 5) against migrated PostgreSQL:
//! individual marks of released/future/undated/special episodes, library auto-add rules,
//! revisions, idempotency, history erase and conflict-safe Undo with owner isolation.

use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use scenecask_api::modules::{library, tracking};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, TestUser, assert_csrf_required, assert_error, count, member_app, released, seed_show,
    send, tracking_state, until_blocked,
};

type Response = (StatusCode, HeaderMap, Value);

struct Fixture {
    app: Router,
    pool: PgPool,
    test: common::TestApp,
    ana: TestUser,
}

async fn fixture(pool: PgPool) -> Fixture {
    let (test, app, ana) = member_app(&pool, |security| {
        tracking::routes(security.clone()).merge(library::routes(security.clone()))
    })
    .await;
    Fixture {
        app,
        pool,
        test,
        ana,
    }
}

impl Fixture {
    async fn send(
        &self,
        user: &TestUser,
        method: Method,
        path: &str,
        key: &str,
        body: Value,
    ) -> Response {
        let body = body.to_string();
        send(
            &self.app,
            Call::write(method, path, &body)
                .idempotent(key)
                .signed(&user.signed),
        )
        .await
    }

    /// Ana marks `episode` with a fresh key and expects success.
    async fn mark(&self, episode: Uuid, watched: bool, expected_revision: i64) -> Value {
        let (status, _, body) = self
            .mark_as(&self.ana, &key(), episode, watched, expected_revision)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn mark_as(
        &self,
        user: &TestUser,
        key: &str,
        episode: Uuid,
        watched: bool,
        expected_revision: i64,
    ) -> Response {
        let path = format!("/progress/episodes/{episode}");
        let body = json!({"watched": watched, "expectedRevision": expected_revision});
        self.send(user, Method::PUT, &path, key, body).await
    }

    async fn erase(&self, show: Uuid, expected_revision: i64) -> Response {
        let path = format!("/shows/{show}/history");
        let body = json!({"expectedRevision": expected_revision});
        self.send(&self.ana, Method::DELETE, &path, &key(), body)
            .await
    }

    async fn undo_as(&self, user: &TestUser, key: &str, action: &Value) -> Response {
        let path = format!("/actions/{}/undo", action.as_str().unwrap());
        self.send(user, Method::POST, &path, key, json!({})).await
    }

    /// Ana undoes `action` with a fresh key and expects success.
    async fn undo(&self, action: &Value) -> Value {
        let (status, _, body) = self.undo_as(&self.ana, &key(), action).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Starts Ana's write in the background, for tests that hold a lock the write must wait on.
    fn spawn(
        &self,
        method: Method,
        path: String,
        body: Value,
    ) -> tokio::task::JoinHandle<Response> {
        let app = self.app.clone();
        let (cookie, csrf) = (self.ana.signed.cookie.clone(), self.ana.signed.csrf.clone());
        tokio::spawn(async move {
            let (body, k) = (body.to_string(), key());
            let mut call = Call::write(method, &path, &body).idempotent(&k);
            call.cookie = Some(&cookie);
            call.csrf = Some(&csrf);
            send(&app, call).await
        })
    }

    async fn progress(&self, episode: Uuid) -> (bool, i64) {
        sqlx::query_as(
            "SELECT watched, revision FROM episode_progress WHERE episode_id = $1 AND user_id = $2",
        )
        .bind(episode)
        .bind(self.ana.id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn state(&self) -> Value {
        tracking_state(&self.pool).await
    }

    /// Ana puts a saved show On hold.
    async fn hold(&self, show: Uuid, expected_revision: i64) {
        let path = format!("/library/{show}");
        let body = json!({"status": "on_hold", "expectedRevision": expected_revision});
        let (status, _, body) = self
            .send(&self.ana, Method::PATCH, &path, &key(), body)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn other_user(&self) -> TestUser {
        TestUser::create(&self.test.security, true, "member").await
    }
}

fn key() -> String {
    Uuid::new_v4().to_string()
}

const FUTURE: Option<&str> = Some("2999-01-01");

/// Inserts an activity event the social projection would write for `action`.
async fn activity(pool: &PgPool, actor: Uuid, show: Uuid, episode: Option<Uuid>, action: &Value) {
    let action: Uuid = action.as_str().unwrap().parse().unwrap();
    let kind = if episode.is_some() {
        "episode_watched"
    } else {
        "library_added"
    };
    sqlx::query(
        "INSERT INTO activity_events (actor_id, kind, show_id, episode_id, action_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(actor)
    .bind(kind)
    .bind(show)
    .bind(episode)
    .bind(action)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn future_mark_persists_keeps_release_and_undo_reverses_only_its_fields(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), released(1, 2), (1, 3, FUTURE)],
    )
    .await;
    let catalog_before = f.state().await["catalog"].clone();

    // C09 case 2: marking future E3 persists it, saves the show as Watching and keeps the
    // release metadata; next still skips to the first released unmarked episode.
    let future = f.mark(eps[2], true, 0).await;
    assert_eq!(future["changed"], 1);
    assert_eq!(
        future["episodes"],
        json!([{"id": eps[2], "watched": true, "revision": 1}])
    );
    assert_eq!(future["trackingRevision"], 1);
    let library = &future["library"];
    assert_eq!(
        (&library["saved"], &library["status"], &library["revision"]),
        (&json!(true), &json!("watching"), &json!(1))
    );
    assert_eq!(library["progress"]["watched"], 1);
    assert_eq!(library["progress"]["nextEpisode"]["id"], json!(eps[0]));
    assert_eq!(library["progress"]["state"], "in_progress");
    assert_eq!(f.state().await["catalog"], catalog_before);
    for text in ["Protected", "protected-episode"] {
        assert!(
            !future.to_string().contains(text),
            "no episode details in writes"
        );
    }

    // The auto-add and the mark share one action.
    let action: Uuid = future["actionId"].as_str().unwrap().parse().unwrap();
    let changes: Vec<(String, String)> = sqlx::query_as(
        "SELECT entity, field FROM mutation_changes WHERE action_id = $1 ORDER BY entity, field",
    )
    .bind(action)
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        changes,
        [
            ("episode_progress".into(), "watched".into()),
            ("library_entry".into(), "saved".into()),
            ("library_entry".into(), "saved_at".into()),
            ("library_entry".into(), "status".into()),
        ]
    );

    // C09 case 1: marking E2 with E1 unmarked keeps E1 next and is out of order.
    let e2 = f.mark(eps[1], true, 0).await;
    assert_eq!(e2["library"]["revision"], 1, "already saved and watching");
    assert_eq!(
        e2["library"]["progress"]["nextEpisode"]["id"],
        json!(eps[0])
    );
    assert_eq!(e2["library"]["progress"]["outOfOrder"], true);
    assert_eq!(e2["trackingRevision"], 2);

    // Undo of the future mark reverses only that action's fields: E2 stays watched.
    activity(&f.pool, f.ana.id, show, Some(eps[2]), &future["actionId"]).await;
    activity(&f.pool, f.ana.id, show, Some(eps[1]), &e2["actionId"]).await;
    let undone = f.undo(&future["actionId"]).await;
    assert_eq!(
        (&undone["actionId"], &undone["reverted"], &undone["skipped"]),
        (&future["actionId"], &json!(2), &json!(0))
    );
    assert_eq!(undone["library"]["saved"], false);
    assert_eq!(undone["library"]["status"], "plan_to_watch");
    assert_eq!(undone["library"]["revision"], 2, "reversal advances");
    assert_eq!(undone["library"]["progress"]["watched"], 1);
    assert_eq!(
        f.progress(eps[2]).await,
        (false, 2),
        "false row keeps its revision"
    );
    assert_eq!(f.progress(eps[1]).await, (true, 1));
    assert_eq!(
        count(&f.pool, "SELECT revision FROM tracking_show_state").await,
        3
    );
    assert_eq!(f.state().await["catalog"], catalog_before);
    let events: Vec<Uuid> = sqlx::query_scalar("SELECT episode_id FROM activity_events")
        .fetch_all(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        events,
        [eps[1]],
        "only the reversed mark's activity is removed"
    );
}

#[sqlx::test]
async fn undo_skips_a_field_later_actions_own_even_with_the_same_value(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    let path = format!("/library/{show}");
    let saved = json!({"saved": true, "status": "watching", "expectedRevision": 0});
    let (status, _, _) = f.send(&f.ana, Method::PUT, &path, &key(), saved).await;
    assert_eq!(status, StatusCode::OK);

    // C09 case 5 / acceptance: A true, B false, C true. Undo A skips C's current field.
    let a = f.mark(eps[0], true, 0).await;
    assert_eq!(
        a["library"]["revision"], 1,
        "no library change, so A owns only E1"
    );
    let b = f.mark(eps[0], false, 1).await;
    assert_eq!(b["library"]["saved"], true, "false keeps the show");
    f.mark(eps[0], true, 2).await;
    let before = f.state().await;
    let undone = f.undo(&a["actionId"]).await;
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(0), &json!(1))
    );
    assert_eq!(f.progress(eps[0]).await, (true, 3));
    let after = f.state().await;
    assert_eq!(after["progress"], before["progress"]);
    assert_eq!(
        after["tracking"], before["tracking"],
        "no reversal, no revision"
    );

    // Undo of the latest action does revert, at a new revision.
    let c_action: Value = sqlx::query_scalar(
        "SELECT to_jsonb(id) FROM mutation_actions WHERE kind = 'episode_watched'
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let undone = f.undo(&c_action).await;
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(1), &json!(0))
    );
    assert_eq!(f.progress(eps[0]).await, (false, 4));
}

#[sqlx::test]
async fn library_status_rules_for_true_and_false_marks(pool: PgPool) {
    let f = fixture(pool).await;
    let undated = (1, 2, None);
    let special = (0, 1, Some("2024-01-01"));
    let (_, eps) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), undated, special],
    )
    .await;

    // False never adds the show, and an unchanged value is a no-op without an action.
    let noop = f.mark(eps[0], false, 0).await;
    assert_eq!(
        (&noop["changed"], &noop["actionId"], &noop["undoUntil"]),
        (&json!(0), &Value::Null, &Value::Null)
    );
    assert_eq!(
        noop["episodes"],
        json!([{"id": eps[0], "watched": false, "revision": 0}])
    );
    assert_eq!(noop["library"]["saved"], false);
    assert_eq!(noop["trackingRevision"], 0);
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM mutation_actions").await,
        0
    );
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM library_entries").await,
        0
    );

    // Undated and special episodes are accepted individually.
    let marked = f.mark(eps[1], true, 0).await;
    assert_eq!(marked["library"]["status"], "watching");
    let special = f.mark(eps[2], true, 0).await;
    assert_eq!(special["changed"], 1);
    assert_eq!(
        special["library"]["progress"]["watched"], 1,
        "specials excluded"
    );
    let repeat = f.mark(eps[2], true, 1).await;
    assert_eq!(
        (&repeat["changed"], &repeat["actionId"]),
        (&json!(0), &Value::Null)
    );

    // On hold is preserved by a true mark.
    let show: Uuid = marked["library"]["showId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    f.hold(show, 1).await;
    let path = format!("/library/{show}");
    let held = f.mark(eps[0], true, 0).await;
    assert_eq!(
        (&held["library"]["status"], &held["library"]["revision"]),
        (&json!("on_hold"), &json!(2))
    );

    // A removed Dropped show is saved again and stays Dropped.
    let body = json!({"saved": true, "status": "dropped", "expectedRevision": 2});
    let (status, _, _) = f.send(&f.ana, Method::PUT, &path, &key(), body).await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({"saved": false, "expectedRevision": 3});
    let (status, _, _) = f.send(&f.ana, Method::PUT, &path, &key(), body).await;
    assert_eq!(status, StatusCode::OK);
    let unmarked = f.mark(eps[0], false, 1).await;
    assert_eq!(
        unmarked["library"]["saved"], false,
        "false never adds the show"
    );
    let kept = f.mark(eps[0], true, 2).await;
    assert_eq!(
        (&kept["library"]["saved"], &kept["library"]["status"]),
        (&json!(true), &json!("dropped"))
    );
}

#[sqlx::test]
async fn replays_conflicts_and_validation_change_nothing(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), released(1, 2)],
    )
    .await;
    sqlx::query("UPDATE episodes SET archived_at = now() WHERE id = $1")
        .bind(eps[1])
        .execute(&f.pool)
        .await
        .unwrap();

    // A lost response is retried with the same key: same body, one action, one revision.
    let k = key();
    let (status, _, first) = f.mark_as(&f.ana, &k, eps[0], true, 0).await;
    assert_eq!(status, StatusCode::OK);
    let after_first = f.state().await;
    let (status, _, replay) = f.mark_as(&f.ana, &k, eps[0], true, 0).await;
    assert_eq!((status, &replay), (StatusCode::OK, &first));
    assert_eq!(f.state().await, after_first);
    let (status, headers, body) = f.mark_as(&f.ana, &k, eps[0], false, 1).await;
    assert_error(status, &headers, &body, 409, "REVISION_CONFLICT");

    for (episode, watched, revision, expected, code) in [
        (eps[0], false, 0, 409, "REVISION_CONFLICT"),
        (eps[0], true, 2, 409, "REVISION_CONFLICT"),
        (eps[1], true, 0, 404, "NOT_FOUND"),
        (Uuid::new_v4(), true, 0, 404, "NOT_FOUND"),
        (eps[0], false, -1, 422, "VALIDATION_ERROR"),
    ] {
        let (status, headers, body) = f.mark_as(&f.ana, &key(), episode, watched, revision).await;
        assert_error(status, &headers, &body, expected, code);
    }
    for (path, body, expected) in [
        (
            format!("/progress/episodes/{}", eps[0]),
            json!({"watched": false, "expectedRevision": 1, "extra": true}),
            422,
        ),
        (
            format!("/progress/episodes/{}", eps[0]),
            json!({"expectedRevision": 1}),
            422,
        ),
        (
            "/progress/episodes/not-an-id".into(),
            json!({"watched": false, "expectedRevision": 1}),
            404,
        ),
        (
            format!("/shows/{show}/history"),
            json!({"expectedRevision": 0}),
            409,
        ),
        (
            format!("/shows/{}/history", Uuid::new_v4()),
            json!({"expectedRevision": 0}),
            404,
        ),
        (format!("/shows/{show}/history"), json!({}), 422),
        (format!("/actions/{}/undo", Uuid::new_v4()), json!({}), 404),
        (
            format!("/actions/{}/undo", first["actionId"].as_str().unwrap()),
            json!({"x": 1}),
            422,
        ),
    ] {
        let method = if path.contains("history") {
            Method::DELETE
        } else if path.contains("undo") {
            Method::POST
        } else {
            Method::PUT
        };
        let (status, headers, body) = f.send(&f.ana, method, &path, &key(), body).await;
        assert_eq!(status.as_u16(), expected, "{path}: {body}");
        assert_error(
            status,
            &headers,
            &body,
            expected,
            body["error"]["code"].as_str().unwrap(),
        );
    }

    // Idempotency-Key is required on every tracking write.
    for (method, path) in [
        (Method::PUT, format!("/progress/episodes/{}", eps[0])),
        (Method::DELETE, format!("/shows/{show}/history")),
        (
            Method::POST,
            format!("/actions/{}/undo", first["actionId"].as_str().unwrap()),
        ),
    ] {
        let body = json!({"watched": false, "expectedRevision": 1}).to_string();
        let call = Call::write(method, &path, &body).signed(&f.ana.signed);
        let (status, headers, body) = send(&f.app, call).await;
        assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    }
    assert_eq!(f.state().await, after_first);
}

#[sqlx::test]
async fn history_erase_uses_the_aggregate_revision_and_undo_restores_it(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "ended",
        &[
            released(1, 1),
            released(1, 2),
            (1, 3, FUTURE),
            (0, 1, Some("2024-01-01")),
        ],
    )
    .await;
    for episode in [eps[0], eps[2], eps[3], eps[1]] {
        f.mark(episode, true, 0).await;
    }
    f.mark(eps[1], false, 1).await;
    f.hold(show, 1).await;

    let (status, headers, body) = f.erase(show, 4).await;
    assert_error(status, &headers, &body, 409, "REVISION_CONFLICT");
    let (status, _, erased) = f.erase(show, 5).await;
    assert_eq!(status, StatusCode::OK, "{erased}");
    assert_eq!(erased["changed"], 3, "specials and future marks included");
    assert_eq!(erased["trackingRevision"], 6);
    let mut expected: Vec<Value> = [eps[0], eps[2], eps[3]]
        .iter()
        .map(|id| json!({"id": id, "watched": false, "revision": 2}))
        .collect();
    expected.sort_by_key(|e| e["id"].as_str().unwrap().to_owned());
    assert_eq!(erased["episodes"], json!(expected));
    let library = &erased["library"];
    assert_eq!(
        (&library["saved"], &library["status"], &library["revision"]),
        (&json!(true), &json!("on_hold"), &json!(2))
    );
    assert_eq!(library["progress"]["watched"], 0);
    assert_eq!(
        count(
            &f.pool,
            "SELECT count(*) FROM episode_progress WHERE NOT watched"
        )
        .await,
        4,
        "false rows are kept"
    );

    // Nothing left to erase is a no-op.
    let (_, _, again) = f.erase(show, 6).await;
    assert_eq!(
        (&again["changed"], &again["actionId"]),
        (&json!(0), &Value::Null)
    );

    // A later re-mark is skipped; the rest of the erase is reverted with new revisions.
    f.mark(eps[3], true, 2).await;
    let undone = f.undo(&erased["actionId"]).await;
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(2), &json!(1))
    );
    assert_eq!(f.progress(eps[0]).await, (true, 3));
    assert_eq!(f.progress(eps[2]).await, (true, 3));
    assert_eq!(f.progress(eps[3]).await, (true, 3));
    assert_eq!(f.progress(eps[1]).await, (false, 2));
    assert_eq!(undone["library"]["status"], "on_hold");
    assert_eq!(
        undone["library"]["progress"]["watched"], 2,
        "E1 and the individually marked future E3; the special is excluded"
    );
}

#[sqlx::test]
async fn undo_is_owner_only_repeatable_and_expires(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    let ben = f.other_user().await;
    let marked = f.mark(eps[0], true, 0).await;
    activity(&f.pool, f.ana.id, show, None, &marked["actionId"]).await;

    // Another user's independent progress and their attempt to undo Ana's action.
    let (status, _, bens) = f.mark_as(&ben, &key(), eps[0], true, 0).await;
    assert_eq!(
        (status, &bens["episodes"][0]["revision"]),
        (StatusCode::OK, &json!(1))
    );
    let before = f.state().await;
    let (status, headers, body) = f.undo_as(&ben, &key(), &marked["actionId"]).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    assert_eq!(f.state().await, before);

    // Repeats with the same or a new key return the stored outcome without new writes.
    let k = key();
    let (status, _, first) = f.undo_as(&f.ana, &k, &marked["actionId"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&first["reverted"], &first["skipped"]),
        (&json!(2), &json!(0))
    );
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM activity_events").await,
        0
    );
    let after = f.state().await;
    for k in [k, key()] {
        let (status, _, repeat) = f.undo_as(&f.ana, &k, &marked["actionId"]).await;
        assert_eq!((status, &repeat), (StatusCode::OK, &first));
    }
    let mut repeated = f.state().await;
    repeated["keys"] = after["keys"].clone();
    assert_eq!(repeated, after);

    // Past its 10-minute window an action that was never undone is 410.
    let later = f.mark(eps[0], true, 2).await;
    sqlx::query(
        "UPDATE mutation_actions SET created_at = created_at - interval '11 minutes',
                                     undo_until = undo_until - interval '11 minutes'
         WHERE id = $1",
    )
    .bind(later["actionId"].as_str().unwrap().parse::<Uuid>().unwrap())
    .execute(&f.pool)
    .await
    .unwrap();
    let before = f.state().await;
    let (status, headers, body) = f.undo_as(&f.ana, &key(), &later["actionId"]).await;
    assert_error(status, &headers, &body, 410, "ACTION_EXPIRED");
    assert_eq!(f.state().await, before);

    // Library actions share the engine.
    let path = format!("/library/{show}");
    let body = json!({"saved": false, "expectedRevision": 3});
    let (status, _, removed) = f.send(&f.ana, Method::PUT, &path, &key(), body).await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    let restored = f.undo(&removed["actionId"]).await;
    assert_eq!(restored["library"]["saved"], true);
    assert_eq!(restored["library"]["revision"], 5);
}

#[sqlx::test]
async fn concurrent_writes_serialize_per_show(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, eps) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), released(1, 2)],
    )
    .await;

    // Two first marks of one episode: one succeeds, the other sees the new revision.
    let (k1, k2) = (key(), key());
    let mark = |k| f.mark_as(&f.ana, k, eps[0], true, 0);
    let ((a, _, _), (b, _, _)) = tokio::join!(mark(&k1), mark(&k2));
    let mut statuses = [a.as_u16(), b.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);

    // Different episodes of one show both apply, each advancing the aggregate revision once.
    let marked = f.mark(eps[1], true, 0).await;
    assert_eq!(marked["trackingRevision"], 2);

    // Concurrent undos with different keys revert once and report one outcome.
    let (k3, k4) = (key(), key());
    let undo = |k| f.undo_as(&f.ana, k, &marked["actionId"]);
    let ((a, _, x), (b, _, y)) = tokio::join!(undo(&k3), undo(&k4));
    assert_eq!((a, b), (StatusCode::OK, StatusCode::OK));
    assert_eq!(x, y);
    assert_eq!(f.progress(eps[1]).await, (false, 2));
    assert_eq!(
        count(&f.pool, "SELECT revision FROM tracking_show_state").await,
        3
    );
}

#[sqlx::test]
async fn undo_waiting_on_a_later_write_skips_what_it_committed(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    let path = format!("/library/{show}");
    let body = json!({"saved": true, "status": "watching", "expectedRevision": 0});
    f.send(&f.ana, Method::PUT, &path, &key(), body).await;
    let marked = f.mark(eps[0], true, 0).await;

    // A later write holds the show's tracking lock while Undo starts, then commits a change.
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM tracking_show_state WHERE user_id = $1 FOR UPDATE")
        .bind(f.ana.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let path = format!("/actions/{}/undo", marked["actionId"].as_str().unwrap());
    let task = f.spawn(Method::POST, path, json!({}));
    until_blocked(&f.pool, &task).await;
    sqlx::query(
        "UPDATE episode_progress SET watched = true, revision = revision + 1 WHERE episode_id = $1",
    )
    .bind(eps[0])
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, _, undone) = task.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(0), &json!(1))
    );
    assert_eq!(f.progress(eps[0]).await, (true, 2));
}

#[sqlx::test]
async fn first_mark_upgrades_an_entry_a_concurrent_first_save_inserted(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;

    // A first library save has inserted the entry but not committed when the mark starts.
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
         VALUES ($1, $2, true, 'plan_to_watch', now(), 1)",
    )
    .bind(f.ana.id)
    .bind(show)
    .execute(&mut *tx)
    .await
    .unwrap();
    let path = format!("/progress/episodes/{}", eps[0]);
    let body = json!({"watched": true, "expectedRevision": 0});
    let task = f.spawn(Method::PUT, path, body);
    until_blocked(&f.pool, &task).await;
    tx.commit().await.unwrap();

    let (status, _, marked) = task.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{marked}");
    assert_eq!(
        (&marked["library"]["status"], &marked["library"]["revision"]),
        (&json!("watching"), &json!(2))
    );
    assert_eq!(f.progress(eps[0]).await, (true, 1));
}

#[sqlx::test]
async fn tracking_requires_a_verified_session_and_csrf(pool: PgPool) {
    let f = fixture(pool).await;
    let pending = TestUser::create(&f.test.security, false, "member").await;
    let (_, eps) = seed_show(&f.pool, "Hollow Orchard", "returning", &[released(1, 1)]).await;
    let (status, headers, body) = f.mark_as(&pending, &key(), eps[0], true, 0).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");

    let path = format!("/progress/episodes/{}", eps[0]);
    let request = json!({"watched": true, "expectedRevision": 0}).to_string();
    let k = key();
    let call = Call::write(Method::PUT, &path, &request).idempotent(&k);
    let (status, headers, body) = send(&f.app, call).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    assert_csrf_required(&f.app, &f.ana, Method::PUT, &path, &request).await;
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM episode_progress").await,
        0
    );
}
