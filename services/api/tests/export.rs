//! R35 export evidence (C08) against migrated PostgreSQL and a private artifact directory: one
//! active job per user, owner-only status and download, the content allowlist against seeded
//! secrets and other people's data, lease recovery after a crash, retry after a storage failure,
//! and expiry deleting the file before its row.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::http::{Method, StatusCode, header};
use chrono::{DateTime, TimeDelta, Utc};
use scenecask_api::modules::{
    account::export::{self, ExportStore, Exporter, LEASE, MAX_ATTEMPTS, Pass, TTL},
    catalog::refresh::Clock,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, ManualClock, MemberFixture, TestUser, add_password, assert_csrf_required, assert_error,
    checked, count, disable, member_fixture, released, seed_show, send,
};

struct World {
    f: MemberFixture,
    root: PathBuf,
    clock: Arc<ManualClock>,
    exporter: Exporter<Arc<ManualClock>>,
}

impl World {
    async fn new(pool: PgPool) -> Self {
        let root = std::env::temp_dir().join(format!("scenecask-export-{}", Uuid::new_v4()));
        let store = ExportStore::open(&root).unwrap();
        let routes_store = store.clone();
        let f = member_fixture(pool, move |security| {
            export::routes(security.clone(), routes_store)
        })
        .await;
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
            .fetch_one(&f.pool)
            .await
            .unwrap();
        let clock = Arc::new(ManualClock::new(now));
        let exporter = Exporter::new(f.pool.clone(), store, Arc::clone(&clock));
        Self {
            f,
            root,
            clock,
            exporter,
        }
    }

    async fn request(&self, user: &TestUser) -> Uuid {
        let (status, body) = checked(
            &self.f.app,
            Call::post("/me/export", "{}").signed(&user.signed),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["state"], "queued");
        assert_eq!(body.as_object().unwrap().len(), 2);
        body["jobId"].as_str().unwrap().parse().unwrap()
    }

    async fn status(&self, user: &TestUser, job: Uuid) -> (StatusCode, Value) {
        let path = format!("/me/export/{job}");
        checked(&self.f.app, Call::get(&path).signed(&user.signed)).await
    }

    async fn download(
        &self,
        user: &TestUser,
        job: Uuid,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let path = format!("/me/export/{job}/download");
        send(&self.f.app, Call::get(&path).signed(&user.signed)).await
    }

    async fn assert_download_error(&self, user: &TestUser, job: Uuid, status: u16, code: &str) {
        let (actual, headers, body) = self.download(user, job).await;
        assert_error(actual, &headers, &body, status, code);
    }

    async fn run(&self) -> Pass {
        self.exporter.run_once().await.unwrap()
    }

    async fn job(&self, job: Uuid) -> (String, i32, Option<String>) {
        sqlx::query_as("SELECT state, attempts, result_path FROM jobs WHERE id = $1")
            .bind(job)
            .fetch_one(&self.f.pool)
            .await
            .unwrap()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.root)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn exec(pool: &PgPool, sql: &str, ids: &[Uuid]) {
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
    for id in ids {
        query = query.bind(*id);
    }
    query.execute(pool).await.unwrap();
}

async fn profile(pool: &PgPool, id: Uuid, handle: &str) {
    exec(
        pool,
        &format!("UPDATE users SET handle = '{handle}' WHERE id = $1"),
        &[id],
    )
    .await;
}

fn backdate(path: &Path, by: Duration) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

#[sqlx::test]
async fn requests_are_single_active_and_owner_only(pool: PgPool) {
    let w = World::new(pool).await;
    let ana = &w.f.ana;
    let other = TestUser::create(&w.f.test.security, true, "member").await;

    let (first, second) = tokio::join!(w.request(ana), w.request(ana));
    assert_eq!(first, second, "one active export per user");
    assert_eq!(
        count(&w.f.pool, "SELECT count(*) FROM jobs WHERE kind = 'export'").await,
        1
    );
    let (status, body) = w.status(ana, first).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"state": "queued", "expiresAt": null}));
    w.assert_download_error(ana, first, 409, "REVISION_CONFLICT")
        .await;

    // Another user's job, an unknown job and a malformed ID are indistinguishable.
    for path_id in [first.to_string(), Uuid::new_v4().to_string(), "nope".into()] {
        for path in [
            format!("/me/export/{path_id}"),
            format!("/me/export/{path_id}/download"),
        ] {
            let (status, headers, body) =
                send(&w.f.app, Call::get(&path).signed(&other.signed)).await;
            assert_error(status, &headers, &body, 404, "NOT_FOUND");
        }
    }
    let theirs = w.request(&other).await;
    assert_ne!(theirs, first);

    // Contract guards: CSRF, verified email, empty body, and sign-in.
    assert_csrf_required(&w.f.app, ana, Method::POST, "/me/export", "{}").await;
    let unverified = TestUser::create(&w.f.test.security, false, "member").await;
    let (status, headers, body) = send(
        &w.f.app,
        Call::post("/me/export", "{}").signed(&unverified.signed),
    )
    .await;
    assert_error(status, &headers, &body, 403, "EMAIL_UNVERIFIED");
    let (status, headers, body) = send(
        &w.f.app,
        Call::post("/me/export", r#"{"format":"csv"}"#).signed(&ana.signed),
    )
    .await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    let (status, headers, body) = send(&w.f.app, Call::get(&format!("/me/export/{first}"))).await;
    assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");

    // Once ready the job is no longer active, so a new request queues a fresh one.
    assert_eq!(w.run().await.exported, 2);
    let next = w.request(ana).await;
    assert_ne!(next, first);
}

#[sqlx::test]
async fn ready_export_holds_own_data_only(pool: PgPool) {
    let w = World::new(pool).await;
    let db = &w.f.pool;
    let ana = w.f.ana.id;
    let security = &w.f.test.security;
    let [bob, cara, eve, frank] = [
        TestUser::create(security, true, "member").await.id,
        TestUser::create(security, true, "member").await.id,
        TestUser::create(security, true, "member").await.id,
        TestUser::create(security, true, "member").await.id,
    ];
    profile(db, ana, "ana_r").await;
    profile(db, cara, "cara_c").await;

    // Credentials, sessions, tokens and identities that must never be exported.
    add_password(db, ana).await;
    exec(
        db,
        "INSERT INTO auth_tokens (token_hash, user_id, purpose, expires_at)
         VALUES ('\\x5ec7e75ec7e75ec7e7'::bytea, $1, 'reset', now() + interval '1 hour')",
        &[ana],
    )
    .await;
    exec(
        db,
        "INSERT INTO external_identities (user_id, issuer, subject)
         VALUES ($1, 'https://accounts.google.com', 'secret-value-subject')",
        &[ana],
    )
    .await;

    // A saved show with watched, unwatched (false row) and archived watched episodes, and an
    // unsaved show that only has history.
    let (saved, saved_eps) = seed_show(
        db,
        "Saved Show",
        "returning",
        &[released(1, 1), released(1, 2), released(1, 3)],
    )
    .await;
    let (history, history_eps) = seed_show(db, "History Show", "ended", &[released(1, 1)]).await;
    exec(
        db,
        "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
         VALUES ($1, $2, true, 'watching', now(), 2)",
        &[ana, saved],
    )
    .await;
    for (episode, watched) in [
        (saved_eps[0], true),
        (saved_eps[1], false),
        (saved_eps[2], true),
        (history_eps[0], true),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO episode_progress (user_id, episode_id, watched, revision)
                 VALUES ($1, $2, {watched}, 1)"
            ),
            &[ana, episode],
        )
        .await;
    }
    exec(
        db,
        "UPDATE episodes SET archived_at = now() WHERE id = $1",
        &[saved_eps[2]],
    )
    .await;

    // Ana's live and removed comments beside Bob's in Bob's discussion.
    let discussion: Uuid = sqlx::query_scalar(
        "INSERT INTO discussions (host_id, episode_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(bob)
    .bind(saved_eps[0])
    .fetch_one(db)
    .await
    .unwrap();
    let comment = |author: Uuid, body: Option<&'static str>| async move {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO comments (discussion_id, author_id, body, removed_at)
             VALUES ($1, $2, $3, CASE WHEN $3 IS NULL THEN now() END) RETURNING id",
        )
        .bind(discussion)
        .bind(author)
        .bind(body)
        .fetch_one(db)
        .await
        .unwrap()
    };
    let live = comment(ana, Some("Ana's own words")).await;
    let removed = comment(ana, None).await;
    comment(bob, Some("Bob's private words")).await;

    // Ana follows Bob; Cara asked to follow Ana; Ana blocked Eve; Frank blocked Ana.
    exec(
        db,
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ($1, $2, 'approved')",
        &[ana, bob],
    )
    .await;
    exec(
        db,
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ($1, $2, 'pending')",
        &[cara, ana],
    )
    .await;
    let notification: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications (user_id, kind, actor_id) VALUES ($1, 'follow_request', $2)
         RETURNING id",
    )
    .bind(ana)
    .bind(cara)
    .fetch_one(db)
    .await
    .unwrap();
    exec(
        db,
        "INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)",
        &[ana, eve],
    )
    .await;
    exec(
        db,
        "INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)",
        &[frank, ana],
    )
    .await;

    let job = w.request(&w.f.ana).await;
    let pass = w.run().await;
    assert_eq!((pass.exported, pass.failed), (1, 0));
    let (status, body) = w.status(&w.f.ana, job).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "ready");
    let expires: DateTime<Utc> = body["expiresAt"].as_str().unwrap().parse().unwrap();
    let lag = expires - (w.clock.now() + TTL);
    assert_eq!(
        lag.num_milliseconds(),
        0,
        "expires 24 hours after it was recorded"
    );
    assert_eq!(
        body.as_object().unwrap().len(),
        2,
        "no path or URL in status"
    );

    let (status, headers, export) = w.download(&w.f.ana, job).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"scenecask-export.json\""
    );
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");

    let keys: Vec<&str> = export
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "blocks",
            "comments",
            "follows",
            "library",
            "notifications",
            "profile",
            "progress"
        ]
    );
    let email: String = sqlx::query_scalar("SELECT normalized_email FROM users WHERE id = $1")
        .bind(ana)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(
        export["profile"],
        json!({"id": ana, "email": email, "displayName": "Ana", "handle": "ana_r",
               "visibility": "private", "verified": true})
    );

    let library = export["library"].as_array().unwrap();
    assert_eq!(library.len(), 2);
    let item = |show: Uuid| {
        library
            .iter()
            .find(|item| item["showId"] == json!(show))
            .unwrap()
    };
    let saved_item = item(saved);
    assert_eq!(saved_item["saved"], true);
    assert_eq!(saved_item["status"], "watching");
    assert_eq!(saved_item["revision"], 2);
    // The archived episode stays in history but leaves active progress (C02).
    assert_eq!(saved_item["progress"]["watched"], 1);
    assert_eq!(saved_item["progress"]["total"], 2);
    let history_item = item(history);
    assert_eq!(history_item["saved"], false);
    assert_eq!(history_item["progress"]["watched"], 1);

    let progress: Vec<(Value, Value, Value)> = export["progress"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["episodeId"].clone(),
                row["showId"].clone(),
                row["watched"].clone(),
            )
        })
        .collect();
    let mut expected = vec![
        (json!(saved_eps[0]), json!(saved), json!(true)),
        (json!(saved_eps[1]), json!(saved), json!(false)),
        (json!(saved_eps[2]), json!(saved), json!(true)),
        (json!(history_eps[0]), json!(history), json!(true)),
    ];
    let mut actual = progress;
    let key = |row: &(Value, Value, Value)| row.0.to_string();
    actual.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(actual, expected);

    let comments = export["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    let find = |id: Uuid| comments.iter().find(|c| c["id"] == json!(id)).unwrap();
    assert_eq!(find(live)["body"], "Ana's own words");
    assert_eq!(find(live)["removed"], false);
    assert_eq!(find(live)["discussionId"], json!(discussion));
    assert_eq!(find(removed)["body"], Value::Null);
    assert_eq!(find(removed)["removed"], true);

    let follows: Vec<(Value, Value, Value)> = export["follows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["followerId"].clone(),
                f["followeeId"].clone(),
                f["state"].clone(),
            )
        })
        .collect();
    assert_eq!(follows.len(), 2);
    assert!(follows.contains(&(json!(ana), json!(bob), json!("approved"))));
    assert!(follows.contains(&(json!(cara), json!(ana), json!("pending"))));
    let blocks = export["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["blockerId"], json!(ana));
    assert_eq!(blocks[0]["blockedId"], json!(eve));

    let notifications = export["notifications"].as_array().unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["id"], json!(notification));
    assert_eq!(notifications[0]["kind"], "follow_request");
    assert_eq!(notifications[0]["actor"]["id"], json!(cara));
    assert_eq!(notifications[0]["actor"]["handle"], "cara_c");
    assert_eq!(notifications[0]["readAt"], Value::Null);

    let text = export.to_string();
    for secret in [
        "argon2",
        "secret-value",
        "5ec7e75ec7e75ec7e7",
        "Bob's private words",
        "Protected",
        "/protected-episode.jpg",
        "password",
        "token",
        "session",
        "resultPath",
        w.root.to_str().unwrap(),
        &frank.to_string(),
    ] {
        assert!(!text.contains(secret), "export leaked {secret}");
    }
}

#[sqlx::test]
async fn crashed_claims_recover_without_exposing_their_files(pool: PgPool) {
    let w = World::new(pool).await;
    let job = w.request(&w.f.ana).await;

    // A worker claimed the job, wrote its file and crashed before recording it.
    exec(
        &w.f.pool,
        "UPDATE jobs SET state = 'running', attempts = 1, lease_until = now() + interval '5 minutes'
         WHERE id = $1",
        &[job],
    )
    .await;
    let crashed = ExportStore::file_name(job, 1);
    fs::write(w.root.join(&crashed), b"{\"partial\":").unwrap();
    assert_eq!(
        w.run().await,
        Pass::default(),
        "a live lease is not reclaimed"
    );
    w.assert_download_error(&w.f.ana, job, 409, "REVISION_CONFLICT")
        .await;

    w.clock.advance(LEASE + TimeDelta::seconds(1));
    let pass = w.run().await;
    assert_eq!(pass.exported, 1);
    let recorded = ExportStore::file_name(job, 2);
    assert_eq!(
        w.job(job).await,
        ("ready".into(), 2, Some(recorded.clone()))
    );
    let (status, _, body) = w.download(&w.f.ana, job).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["profile"]["id"], json!(w.f.ana.id));

    // The crashed claim's file is unreachable and swept once its grace period ends.
    assert_eq!(w.files(), {
        let mut both = vec![crashed.clone(), recorded.clone()];
        both.sort();
        both
    });
    backdate(&w.root.join(&crashed), Duration::from_secs(2 * 60 * 60));
    assert_eq!(w.run().await.swept, 1);
    assert_eq!(w.files(), [recorded]);

    // A job that crashed on its last claim fails rather than looping, and frees the user.
    let exhausted = w.request(&w.f.ana).await;
    exec(
        &w.f.pool,
        &format!(
            "UPDATE jobs SET state = 'running', attempts = {MAX_ATTEMPTS}, lease_until = now()
             WHERE id = $1"
        ),
        &[exhausted],
    )
    .await;
    w.clock.advance(LEASE);
    w.run().await;
    assert_eq!(
        w.job(exhausted).await,
        ("failed".into(), MAX_ATTEMPTS, None)
    );
    let (_, body) = w.status(&w.f.ana, exhausted).await;
    assert_eq!(body, json!({"state": "failed", "expiresAt": null}));
    w.assert_download_error(&w.f.ana, exhausted, 409, "REVISION_CONFLICT")
        .await;
    assert_ne!(w.request(&w.f.ana).await, exhausted);
}

#[sqlx::test]
async fn storage_failures_retry_and_disabled_accounts_fail(pool: PgPool) {
    let w = World::new(pool).await;
    let job = w.request(&w.f.ana).await;

    fs::remove_dir_all(&w.root).unwrap();
    let pass = w.run().await;
    assert_eq!((pass.retried, pass.exported), (1, 0));
    assert_eq!(w.job(job).await, ("queued".into(), 1, None));
    let (_, body) = w.status(&w.f.ana, job).await;
    assert_eq!(body["state"], "queued");

    fs::create_dir(&w.root).unwrap();
    assert_eq!(w.run().await.exported, 0, "waits for its backoff");
    w.clock.advance(TimeDelta::minutes(1));
    assert_eq!(w.run().await.exported, 1);
    assert_eq!(w.job(job).await.0, "ready");

    // An account disabled for deletion gets no artifact and is not retried.
    let other = TestUser::create(&w.f.test.security, true, "member").await;
    let theirs = w.request(&other).await;
    disable(&w.f.pool, other.id).await;
    let pass = w.run().await;
    assert_eq!((pass.failed, pass.exported), (1, 0));
    assert_eq!(w.job(theirs).await, ("failed".into(), 1, None));
    assert_eq!(w.files().len(), 1);
}

#[sqlx::test]
async fn expired_artifacts_answer_gone_and_are_deleted_before_their_rows(pool: PgPool) {
    let w = World::new(pool).await;
    let job = w.request(&w.f.ana).await;
    w.run().await;
    let (_, _, path) = w.job(job).await;
    let path = path.unwrap();

    // Expiry is enforced on read even before the worker deletes the file.
    exec(
        &w.f.pool,
        "UPDATE jobs SET expires_at = now() - interval '1 second' WHERE id = $1",
        &[job],
    )
    .await;
    w.assert_download_error(&w.f.ana, job, 410, "ACTION_EXPIRED")
        .await;
    assert_eq!(w.files(), std::slice::from_ref(&path));

    w.clock.advance(TTL + TimeDelta::seconds(1));
    let pass = w.run().await;
    assert_eq!(pass.expired, 1);
    assert!(w.files().is_empty());
    assert_eq!(w.job(job).await, ("ready".into(), 1, None));
    w.assert_download_error(&w.f.ana, job, 410, "ACTION_EXPIRED")
        .await;
    let (status, body) = w.status(&w.f.ana, job).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "ready");
    assert!(body["expiresAt"].is_string());

    // After retention the row goes too; the job then reads as unknown.
    w.clock.advance(TimeDelta::days(8));
    w.run().await;
    assert_eq!(
        count(&w.f.pool, "SELECT count(*) FROM jobs WHERE kind = 'export'").await,
        0
    );
    let (status, _) = w.status(&w.f.ana, job).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
