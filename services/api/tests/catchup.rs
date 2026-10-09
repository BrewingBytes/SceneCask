//! R17 catch-up evidence (C05, C09 case 4) against migrated PostgreSQL: exact preview sets and
//! exclusions, stale/expired/cross-user previews, idempotent atomic commit, no-op previews,
//! commit races and Undo that keeps earlier marks.

use axum::http::{HeaderMap, Method, StatusCode, header};
use scenecask_api::modules::{
    library,
    tracking::{self, catchup},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, TestUser, assert_csrf_required, assert_error, count, member_fixture,
    released, seed_show, send, tracking_state, until_blocked,
};

type Response = (StatusCode, HeaderMap, Value);

const FUTURE: Option<&str> = Some("2999-01-01");

async fn fixture(pool: PgPool) -> MemberFixture {
    member_fixture(pool, |security| {
        catchup::routes(security.clone())
            .merge(tracking::routes(security.clone()))
            .merge(library::routes(security.clone()))
    })
    .await
}

fn key() -> String {
    Uuid::new_v4().to_string()
}

/// C09 case 4 plus a later released episode and a special: E1–E3 released, E4 future, E5
/// undated, E6 released, S0E1 released.
async fn mixed_show(pool: &PgPool) -> (Uuid, Vec<Uuid>) {
    seed_show(
        pool,
        "Hollow Orchard",
        "returning",
        &[
            released(1, 1),
            released(1, 2),
            released(1, 3),
            (1, 4, FUTURE),
            (1, 5, None),
            released(1, 6),
            released(0, 1),
        ],
    )
    .await
}

/// A fixture with the mixed show and Ana's preview through E3 (E1–E3 included).
async fn previewed(pool: PgPool) -> (MemberFixture, Uuid, Vec<Uuid>, Value) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let preview = f.preview(show, eps[2]).await;
    (f, show, eps, preview)
}

fn codes(episodes: &[Uuid], picks: &[(usize, i32, i32)]) -> Value {
    picks
        .iter()
        .map(|&(i, season, episode)| json!({"id": episodes[i], "season": season, "episode": episode}))
        .collect()
}

impl MemberFixture {
    async fn preview_as(&self, user: &TestUser, show: Uuid, through: Uuid) -> Response {
        let path = format!("/shows/{show}/catch-up/preview");
        let body = json!({"throughEpisodeId": through}).to_string();
        send(&self.app, Call::post(&path, &body).signed(&user.signed)).await
    }

    /// Ana previews catching up through `through` and expects success.
    async fn preview(&self, show: Uuid, through: Uuid) -> Value {
        let (status, headers, body) = self.preview_as(&self.ana, show, through).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        body
    }

    async fn commit_as(&self, user: &TestUser, key: &str, show: Uuid, preview: &Value) -> Response {
        let path = format!("/shows/{show}/catch-up");
        let body = json!({"previewId": preview["previewId"]}).to_string();
        send(
            &self.app,
            Call::post(&path, &body)
                .idempotent(key)
                .signed(&user.signed),
        )
        .await
    }

    /// Ana confirms `preview` with a fresh key and expects success.
    async fn commit(&self, show: Uuid, preview: &Value) -> Value {
        let (status, _, body) = self.commit_as(&self.ana, &key(), show, preview).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Ana's keyed write through another tracking or library route; expects success.
    async fn write(&self, method: Method, path: &str, body: Value) -> Value {
        let (body, k) = (body.to_string(), key());
        let call = Call::write(method, path, &body)
            .idempotent(&k)
            .signed(&self.ana.signed);
        let (status, _, body) = send(&self.app, call).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn mark(&self, episode: Uuid) -> Value {
        let path = format!("/progress/episodes/{episode}");
        let body = json!({"watched": true, "expectedRevision": 0});
        self.write(Method::PUT, &path, body).await
    }

    async fn watched(&self, episode: Uuid) -> Option<(bool, i64)> {
        sqlx::query_as(
            "SELECT watched, revision FROM episode_progress WHERE episode_id = $1 AND user_id = $2",
        )
        .bind(episode)
        .bind(self.ana.id)
        .fetch_optional(&self.pool)
        .await
        .unwrap()
    }
}

#[sqlx::test]
async fn preview_lists_the_exact_set_and_undo_of_the_commit_keeps_earlier_marks(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    f.mark(eps[0]).await;
    let before = tracking_state(&f.pool).await;

    // C09 case 4: through undated E5 with E1 marked, only E2/E3 are included.
    let preview = f.preview(show, eps[4]).await;
    assert_eq!(preview["through"], json!({"season": 1, "episode": 5}));
    assert_eq!(preview["included"], codes(&eps, &[(1, 1, 2), (2, 1, 3)]));
    assert_eq!(
        preview["excluded"],
        json!({
            "alreadyWatched": codes(&eps, &[(0, 1, 1)]),
            "future": codes(&eps, &[(3, 1, 4)]),
            "undated": codes(&eps, &[(4, 1, 5)]),
            "specials": codes(&eps, &[(6, 0, 1)]),
        })
    );
    assert_eq!(preview["count"], 2);
    assert!(preview["previewId"].is_string() && preview["expiresAt"].is_string());
    for text in ["Protected", "protected-episode"] {
        assert!(!preview.to_string().contains(text), "no episode details");
    }
    assert_eq!(
        tracking_state(&f.pool).await,
        before,
        "preview writes nothing"
    );
    let expires: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - created_at)::float8 FROM catchup_previews",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(expires, 300.0);

    let committed = f.commit(show, &preview).await;
    assert_eq!(committed["changed"], 2);
    let mut expected = vec![
        json!({"id": eps[1], "watched": true, "revision": 1}),
        json!({"id": eps[2], "watched": true, "revision": 1}),
    ];
    expected.sort_by_key(|e| e["id"].as_str().unwrap().to_owned());
    assert_eq!(committed["episodes"], Value::Array(expected));
    assert_eq!(committed["trackingRevision"], 2, "one bump per commit");
    assert_eq!(committed["library"]["progress"]["watched"], 3);
    assert_eq!(
        committed["library"]["revision"], 1,
        "already saved and watching"
    );
    assert_eq!(f.watched(eps[5]).await, None, "never expanded past the set");
    let kinds: Vec<(String, i64)> = sqlx::query_as(
        "SELECT a.kind, count(c.*) FROM mutation_actions a JOIN mutation_changes c ON c.action_id = a.id
         WHERE a.id = $1::text::uuid GROUP BY a.kind",
    )
    .bind(committed["actionId"].as_str().unwrap())
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(kinds, [("catch_up".to_owned(), 2)]);

    // Undo reverses only the caught-up episodes; individually marked E1 stays watched.
    let path = format!("/actions/{}/undo", committed["actionId"].as_str().unwrap());
    let undone = f.write(Method::POST, &path, json!({})).await;
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(2), &json!(0))
    );
    assert_eq!(f.watched(eps[0]).await, Some((true, 1)));
    assert_eq!(f.watched(eps[1]).await, Some((false, 2)));
    assert_eq!(f.watched(eps[2]).await, Some((false, 2)));
    assert_eq!(undone["library"]["saved"], true);
    assert_eq!(undone["library"]["progress"]["watched"], 1);
}

#[sqlx::test]
async fn future_endpoint_excludes_future_episodes_and_commit_auto_adds(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;

    let preview = f.preview(show, eps[3]).await;
    assert_eq!(
        preview["included"],
        codes(&eps, &[(0, 1, 1), (1, 1, 2), (2, 1, 3)])
    );
    assert_eq!(preview["excluded"]["future"], codes(&eps, &[(3, 1, 4)]));
    assert_eq!(preview["excluded"]["undated"], json!([]));
    assert_eq!(preview["count"], 3);

    let committed = f.commit(show, &preview).await;
    assert_eq!(committed["changed"], 3);
    let library = &committed["library"];
    assert_eq!(
        (&library["saved"], &library["status"], &library["revision"]),
        (&json!(true), &json!("watching"), &json!(1))
    );
    assert_eq!(
        f.watched(eps[3]).await,
        None,
        "future endpoint stays unmarked"
    );

    // Undo removes the auto-add with the marks.
    let path = format!("/actions/{}/undo", committed["actionId"].as_str().unwrap());
    let undone = f.write(Method::POST, &path, json!({})).await;
    assert_eq!(
        (&undone["reverted"], &undone["skipped"]),
        (&json!(4), &json!(0))
    );
    assert_eq!(undone["library"]["saved"], false);
    assert_eq!(undone["library"]["status"], "plan_to_watch");
}

#[sqlx::test]
async fn commit_keeps_on_hold_and_dropped_statuses(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let path = format!("/library/{show}");
    let saved = json!({"saved": true, "status": "on_hold", "expectedRevision": 0});
    f.write(Method::PUT, &path, saved).await;

    let committed = f.commit(show, &f.preview(show, eps[1]).await).await;
    assert_eq!(committed["changed"], 2);
    assert_eq!(committed["library"]["status"], "on_hold");
    assert_eq!(committed["library"]["revision"], 1, "no library change");
}

#[sqlx::test]
async fn endpoint_must_be_an_active_regular_episode_of_the_show(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let (_, other) = seed_show(&f.pool, "Other", "ended", &[released(1, 1)]).await;
    sqlx::query("UPDATE episodes SET archived_at = now() WHERE id = $1")
        .bind(eps[5])
        .execute(&f.pool)
        .await
        .unwrap();

    for through in [eps[6], other[0], eps[5], Uuid::new_v4()] {
        let (status, headers, body) = f.preview_as(&f.ana, show, through).await;
        assert_error(status, &headers, &body, 422, "INVALID_EPISODE_SCOPE");
    }
    let (status, headers, body) = f.preview_as(&f.ana, Uuid::new_v4(), eps[0]).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let body = json!({"throughEpisodeId": eps[0]}).to_string();
    let call = Call::post("/shows/not-a-uuid/catch-up/preview", &body).signed(&f.ana.signed);
    let (status, headers, body) = send(&f.app, call).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM catchup_previews").await,
        0
    );
}

#[sqlx::test]
async fn metadata_progress_or_library_change_before_confirm_is_stale_with_no_writes(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let library = format!("/library/{show}");
    f.write(
        Method::PUT,
        &library,
        json!({"saved": true, "expectedRevision": 0}),
    )
    .await;

    // Each change touches only E6, outside the previewed set, and still invalidates it.
    for case in ["metadata", "progress", "library"] {
        let preview = f.preview(show, eps[2]).await;
        assert_eq!(preview["count"], 3, "{case}");
        match case {
            "metadata" => {
                sqlx::query("UPDATE episodes SET air_date = '2024-02-01' WHERE id = $1")
                    .bind(eps[5])
                    .execute(&f.pool)
                    .await
                    .unwrap();
            }
            "progress" => {
                f.mark(eps[5]).await;
            }
            _ => {
                let revision = count(&f.pool, "SELECT revision FROM library_entries").await;
                let body = json!({"status": "dropped", "expectedRevision": revision});
                f.write(Method::PATCH, &library, body).await;
            }
        }
        let before = tracking_state(&f.pool).await;
        let (status, headers, body) = f.commit_as(&f.ana, &key(), show, &preview).await;
        assert_error(status, &headers, &body, 409, "PREVIEW_STALE");
        assert_eq!(tracking_state(&f.pool).await, before, "{case}: no writes");
    }

    // A fresh preview after the changes commits.
    let committed = f.commit(show, &f.preview(show, eps[2]).await).await;
    assert_eq!(committed["changed"], 3);
}

#[sqlx::test]
async fn expired_cross_user_and_other_show_previews_are_rejected(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let (other_show, _) = seed_show(&f.pool, "Other", "ended", &[released(1, 1)]).await;
    let preview = f.preview(show, eps[2]).await;
    let before = tracking_state(&f.pool).await;

    let bea = TestUser::create(&f.test.security, true, "member").await;
    let (status, headers, body) = f.commit_as(&bea, &key(), show, &preview).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let (status, headers, body) = f.commit_as(&f.ana, &key(), other_show, &preview).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let unknown = json!({"previewId": Uuid::new_v4()});
    let (status, headers, body) = f.commit_as(&f.ana, &key(), show, &unknown).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");

    sqlx::query(
        "UPDATE catchup_previews SET created_at = now() - interval '6 minutes',
             expires_at = now() - interval '1 minute'",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let (status, headers, body) = f.commit_as(&f.ana, &key(), show, &preview).await;
    assert_error(status, &headers, &body, 410, "ACTION_EXPIRED");
    assert_eq!(tracking_state(&f.pool).await, before, "no writes");

    // Expired previews are pruned a day after expiry, when the user previews again.
    sqlx::query(
        "UPDATE catchup_previews SET created_at = now() - interval '25 hours',
             expires_at = now() - interval '24 hours 55 minutes'",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let fresh = f.preview(show, eps[2]).await;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM catchup_previews")
        .fetch_all(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        ids,
        [fresh["previewId"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap()]
    );
}

#[sqlx::test]
async fn commit_is_idempotent_and_a_preview_commits_once(pool: PgPool) {
    let (f, show, _, preview) = previewed(pool).await;

    let k = key();
    let (status, _, first) = f.commit_as(&f.ana, &k, show, &preview).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let after = tracking_state(&f.pool).await;
    let (status, _, replay) = f.commit_as(&f.ana, &k, show, &preview).await;
    assert_eq!((status, &replay), (StatusCode::OK, &first));
    let other = json!({"previewId": Uuid::new_v4()});
    let (status, headers, body) = f.commit_as(&f.ana, &k, show, &other).await;
    assert_error(status, &headers, &body, 409, "REVISION_CONFLICT");
    let (status, headers, body) = f.commit_as(&f.ana, &key(), show, &preview).await;
    assert_error(status, &headers, &body, 409, "PREVIEW_STALE");
    assert_eq!(tracking_state(&f.pool).await, after);
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM mutation_actions").await,
        1
    );
}

#[sqlx::test]
async fn concurrent_commits_of_one_preview_mark_once(pool: PgPool) {
    let (f, show, eps, preview) = previewed(pool).await;

    let (k1, k2) = (key(), key());
    let ((a, _, x), (b, _, y)) = tokio::join!(
        f.commit_as(&f.ana, &k1, show, &preview),
        f.commit_as(&f.ana, &k2, show, &preview)
    );
    let mut statuses = [a, b];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT], "{x} {y}");
    for episode in &eps[..3] {
        assert_eq!(f.watched(*episode).await, Some((true, 1)));
    }
    assert_eq!(
        count(&f.pool, "SELECT revision FROM tracking_show_state").await,
        1
    );
}

#[sqlx::test]
async fn catalog_change_committed_while_commit_waits_is_stale(pool: PgPool) {
    let (f, show, eps, preview) = previewed(pool).await;

    // A metadata refresh holds the show row, as the importer does, while the commit starts.
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM shows WHERE id = $1 FOR UPDATE")
        .bind(show)
        .execute(&mut *tx)
        .await
        .unwrap();
    let (app, cookie, csrf) = (
        f.app.clone(),
        f.ana.signed.cookie.clone(),
        f.ana.signed.csrf.clone(),
    );
    let (path, body) = (
        format!("/shows/{show}/catch-up"),
        json!({"previewId": preview["previewId"]}).to_string(),
    );
    let task = tokio::spawn(async move {
        let k = key();
        let mut call = Call::post(&path, &body).idempotent(&k);
        (call.cookie, call.csrf) = (Some(cookie.as_str()), Some(csrf.as_str()));
        send(&app, call).await
    });
    until_blocked(&f.pool, &task).await;
    sqlx::query("UPDATE episodes SET air_date = '2024-03-01' WHERE id = $1")
        .bind(eps[1])
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (status, headers, body) = task.await.unwrap();
    assert_error(status, &headers, &body, 409, "PREVIEW_STALE");
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM episode_progress").await,
        0
    );
}

#[sqlx::test]
async fn zero_eligible_preview_commits_as_a_no_op(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    f.mark(eps[0]).await;

    let preview = f.preview(show, eps[0]).await;
    assert_eq!(preview["count"], 0);
    assert_eq!(preview["included"], json!([]));
    assert_eq!(
        preview["excluded"]["alreadyWatched"],
        codes(&eps, &[(0, 1, 1)])
    );
    let before = tracking_state(&f.pool).await;

    let committed = f.commit(show, &preview).await;
    assert_eq!(
        (
            &committed["changed"],
            &committed["actionId"],
            &committed["undoUntil"],
            &committed["episodes"],
            &committed["trackingRevision"],
        ),
        (&json!(0), &Value::Null, &Value::Null, &json!([]), &json!(1))
    );
    let after = tracking_state(&f.pool).await;
    assert_eq!(after["progress"], before["progress"]);
    assert_eq!(after["actions"], before["actions"], "no action for a no-op");
    assert_eq!(after["tracking"], before["tracking"]);
}

#[sqlx::test]
async fn catch_up_requires_a_verified_session_csrf_and_idempotency_key(pool: PgPool) {
    let f = fixture(pool).await;
    let (show, eps) = mixed_show(&f.pool).await;
    let pending = TestUser::create(&f.test.security, false, "member").await;
    let (status, headers, body) = f.preview_as(&pending, show, eps[2]).await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");

    let path = format!("/shows/{show}/catch-up/preview");
    let request = json!({"throughEpisodeId": eps[2]}).to_string();
    let (status, headers, body) = send(&f.app, Call::post(&path, &request)).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    assert_csrf_required(&f.app, &f.ana, Method::POST, &path, &request).await;

    let preview = f.preview(show, eps[2]).await;
    let path = format!("/shows/{show}/catch-up");
    let request = json!({"previewId": preview["previewId"]}).to_string();
    assert_csrf_required(&f.app, &f.ana, Method::POST, &path, &request).await;
    let call = Call::post(&path, &request).signed(&f.ana.signed);
    let (status, headers, body) = send(&f.app, call).await;
    assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM episode_progress").await,
        0
    );
}
