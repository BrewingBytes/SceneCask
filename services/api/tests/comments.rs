//! R31 comment write evidence (C07) against migrated PostgreSQL: plain-text posting with trim and
//! length validation, write-time privacy and spoiler rechecks (including a block that commits
//! while a post waits), the per-user rate limit, author/host/operator removal as tombstones with
//! 404 for everyone else, and per-reporter hiding with one open report per viewer and comment.

use axum::http::{Method, StatusCode};
use scenecask_api::modules::{discussions, moderation};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, TestUser, approve, assert_csrf_required, assert_error,
    assert_rate_limited, block, checked, comment, count, disable, mark, mutual, profile, released,
    seed_show, send, social_fixture, thread, until_blocked,
};

/// Bo hosts a thread on S1E2. Ana and Cy are mutual follows of Bo, and all three have watched
/// S1E1 and S1E2, so the thread is open to each of them.
struct Thread {
    f: MemberFixture,
    bo: TestUser,
    cy: TestUser,
    id: Uuid,
    episodes: Vec<Uuid>,
}

async fn open_thread(pool: PgPool) -> Thread {
    let f = social_fixture(pool, |security| {
        discussions::routes(security.clone()).merge(moderation::routes(security))
    })
    .await;
    let bo = f.member("bo_i", "private").await;
    let cy = f.member("cy_p", "private").await;
    let (_, episodes) = seed_show(
        &f.pool,
        "Hollow Orchard",
        "returning",
        &[released(1, 1), released(1, 2)],
    )
    .await;
    let id = thread(&f.pool, bo.id, episodes[1]).await;
    for user in [f.ana.id, cy.id, bo.id] {
        if user != bo.id {
            mutual(&f.pool, user, bo.id).await;
        }
        mark(&f.pool, user, &episodes).await;
    }
    Thread {
        f,
        bo,
        cy,
        id,
        episodes,
    }
}

impl Thread {
    async fn post(&self, user: &TestUser, body: &str) -> (StatusCode, Value) {
        self.post_raw(user, &json!({ "body": body }).to_string())
            .await
    }

    async fn post_raw(&self, user: &TestUser, request: &str) -> (StatusCode, Value) {
        let path = format!("/discussions/{}/comments", self.id);
        checked(&self.f.app, Call::post(&path, request).signed(&user.signed)).await
    }

    async fn delete(&self, user: &TestUser, comment: &str) -> StatusCode {
        let path = format!("/comments/{comment}");
        let call = Call::write(Method::DELETE, &path, "{}").signed(&user.signed);
        let (status, body) = checked(&self.f.app, call).await;
        if status.is_success() {
            assert!(body.is_null());
        } else {
            assert_eq!(body["error"]["code"], "NOT_FOUND");
        }
        status
    }

    async fn report(&self, user: &TestUser, comment: &str, request: Value) -> (StatusCode, Value) {
        let path = format!("/comments/{comment}/reports");
        let request = request.to_string();
        let call = Call::post(&path, &request).signed(&user.signed);
        checked(&self.f.app, call).await
    }

    /// The comments `user` reads, asserting the thread is open to them.
    async fn read(&self, user: &TestUser) -> Vec<Value> {
        let path = format!("/discussions/{}/comments", self.id);
        let (status, page) = checked(&self.f.app, Call::get(&path).signed(&user.signed)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"].as_array().unwrap().clone()
    }

    async fn comments(&self) -> i64 {
        count(&self.f.pool, "SELECT count(*) FROM comments").await
    }

    /// body, removed_at, removed_by of `comment`.
    async fn stored(&self, comment: Uuid) -> (Option<String>, Option<String>, Option<Uuid>) {
        sqlx::query_as("SELECT body, removed_at::text, removed_by FROM comments WHERE id = $1")
            .bind(comment)
            .fetch_one(&self.f.pool)
            .await
            .unwrap()
    }
}

fn find(items: &[Value], id: Uuid) -> Option<&Value> {
    items.iter().find(|item| item["id"] == id.to_string())
}

fn assert_code(response: (StatusCode, Value), status: StatusCode, code: &str) {
    assert_eq!(response.0, status, "{}", response.1);
    assert_eq!(response.1["error"]["code"], code);
}

/// Acceptance (XSS plain-text payload): markup, Markdown and links are stored and returned as the
/// exact trimmed string, never interpreted, and every reader gets the same text.
#[sqlx::test]
async fn posts_trimmed_plain_text_as_its_author_sees_it(pool: PgPool) {
    let t = open_thread(pool).await;
    let raw = "<script>alert(1)</script> <img src=x onerror=alert(2)> **bold** \
               [x](https://evil.test) &amp; é\nsecond line";

    let (status, posted) = t.post(&t.f.ana, &format!(" \n\t{raw} \n")).await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    let mut keys: Vec<&str> = posted
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "author",
            "body",
            "canDelete",
            "canModerate",
            "createdAt",
            "id",
            "removed"
        ]
    );
    assert_eq!(posted["body"], raw);
    assert_eq!(posted["author"]["handle"], "ana_r");
    assert_eq!(posted["removed"], false);
    assert_eq!(posted["canDelete"], true);
    assert_eq!(posted["canModerate"], false);
    let id: Uuid = posted["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(t.stored(id).await.0.as_deref(), Some(raw));

    let seen = t.read(&t.cy).await;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["body"], raw);
    assert_eq!(seen[0]["author"]["handle"], "ana_r");
    assert_eq!(seen[0]["canDelete"], false);
    assert_eq!(find(&t.read(&t.bo).await, id).unwrap()["canDelete"], true);
}

#[sqlx::test]
async fn rejects_empty_oversized_and_non_text_bodies_without_writes(pool: PgPool) {
    let t = open_thread(pool).await;
    for request in [
        json!({ "body": "" }),
        json!({ "body": " \n\t " }),
        json!({ "body": "x".repeat(5001) }),
        json!({ "body": "é".repeat(5001) }),
        json!({ "body": "before\u{0}after" }),
        json!({ "body": "bell\u{7}" }),
        json!({}),
        json!({ "body": 5 }),
        json!({ "body": "ok", "attachments": [] }),
    ] {
        let (status, body) = t.post_raw(&t.f.ana, &request.to_string()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
        assert!(!body.to_string().contains("before"));
    }
    assert_code(
        t.post_raw(&t.f.ana, "{\"body\":").await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_ERROR",
    );
    assert_eq!(t.comments().await, 0);

    // Limits count Unicode characters after trimming.
    let (status, posted) = t.post(&t.f.ana, &format!("  {}  ", "é".repeat(5000))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(posted["body"].as_str().unwrap().chars().count(), 5000);

    // A comment needs an author card, so a handle.
    sqlx::query("UPDATE users SET handle = NULL WHERE id = $1")
        .bind(t.cy.id)
        .execute(&t.f.pool)
        .await
        .unwrap();
    let (status, body) = t.post(&t.cy, "Hello").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["handle"].is_string());
    assert_eq!(t.comments().await, 1);
}

/// Acceptance: posting to a locked or revoked thread fails without a write. Privacy (404) comes
/// before the spoiler gate (403), and revocation supersedes an earlier reveal.
#[sqlx::test]
async fn locked_or_revoked_threads_reject_posts_without_writes(pool: PgPool) {
    let t = open_thread(pool).await;
    let dee = t.f.member("dee_s", "public").await;
    let concealed = |response| assert_code(response, StatusCode::NOT_FOUND, "NOT_FOUND");
    let locked = |response| assert_code(response, StatusCode::FORBIDDEN, "SPOILER_LOCKED");

    concealed(t.post(&dee, "Hi").await);
    approve(&t.f.pool, dee.id, t.bo.id).await;
    concealed(t.post(&dee, "Hi").await);
    approve(&t.f.pool, t.bo.id, dee.id).await;
    locked(t.post(&dee, "Hi").await);
    // Watching the target alone leaves the S1E1 gap.
    mark(&t.f.pool, dee.id, &t.episodes[1..]).await;
    locked(t.post(&dee, "Hi").await);
    assert_eq!(t.comments().await, 0);

    let body = json!({ "scope": "discussion", "resourceId": t.id }).to_string();
    let (status, _) = checked(&t.f.app, Call::post("/reveals", &body).signed(&dee.signed)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(t.post(&dee, "Hi").await.0, StatusCode::CREATED);

    // Unfollowing revokes access despite the reveal; blocks and a disabled host do too.
    sqlx::query("DELETE FROM follows WHERE follower_id = $1 AND followee_id = $2")
        .bind(t.bo.id)
        .bind(dee.id)
        .execute(&t.f.pool)
        .await
        .unwrap();
    concealed(t.post(&dee, "Hi").await);
    block(&t.f.pool, t.cy.id, t.bo.id).await;
    concealed(t.post(&t.cy, "Hi").await);
    disable(&t.f.pool, t.bo.id).await;
    concealed(t.post(&t.f.ana, "Hi").await);

    for path in [
        format!("/discussions/{}/comments", Uuid::new_v4()),
        "/discussions/not-a-uuid/comments".to_owned(),
    ] {
        let call = Call::post(&path, "{\"body\":\"Hi\"}").signed(&t.f.ana.signed);
        concealed(checked(&t.f.app, call).await);
    }
    assert_eq!(t.comments().await, 1);
}

/// A block between host and poster that commits while the post waits on the pair's rows rejects
/// the post rather than writing past the revocation.
#[sqlx::test]
async fn a_block_committing_during_a_post_rejects_it(pool: PgPool) {
    let t = open_thread(pool).await;
    let mut tx = t.f.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM users WHERE id = ANY($1) ORDER BY id FOR NO KEY UPDATE")
        .bind([t.f.ana.id, t.bo.id])
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)")
        .bind(t.bo.id)
        .bind(t.f.ana.id)
        .execute(&mut *tx)
        .await
        .unwrap();

    let app = t.f.app.clone();
    let path = format!("/discussions/{}/comments", t.id);
    let (cookie, csrf) = (t.f.ana.signed.cookie.clone(), t.f.ana.signed.csrf.clone());
    let task = tokio::spawn(async move {
        let mut call = Call::post(&path, "{\"body\":\"Too late\"}");
        call.cookie = Some(&cookie);
        call.csrf = Some(&csrf);
        send(&app, call).await
    });
    until_blocked(&t.f.pool, &task).await;
    tx.commit().await.unwrap();
    let (status, headers, body) = task.await.unwrap();
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    assert_eq!(t.comments().await, 0);
}

#[sqlx::test]
async fn posting_is_limited_to_ten_per_minute_per_user(pool: PgPool) {
    let t = open_thread(pool).await;
    // Rejected posts do not spend the budget.
    let dee = t.f.member("dee_s", "public").await;
    for _ in 0..3 {
        assert_eq!(t.post(&dee, "Hi").await.0, StatusCode::NOT_FOUND);
    }
    assert_eq!(
        t.post(&t.f.ana, "").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for n in 0..10 {
        let (status, body) = t.post(&t.f.ana, &format!("Comment {n}")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let path = format!("/discussions/{}/comments", t.id);
    let call = Call::post(&path, "{\"body\":\"One more\"}").signed(&t.f.ana.signed);
    let (status, headers, body) = send(&t.f.app, call).await;
    assert_rate_limited(status, &headers, &body);
    assert_eq!(t.comments().await, 10);

    // The limit is per user.
    assert_eq!(t.post(&t.cy, "My turn").await.0, StatusCode::CREATED);
}

/// Acceptance: a host's removal of another's comment drops its body for every reader while the
/// tombstone keeps its place. Participants, other hosts and unknown IDs get 404 and change
/// nothing; authors and operators may remove, and repeats keep the first removal.
#[sqlx::test]
async fn authors_hosts_and_operators_remove_comments_and_no_one_else_can(pool: PgPool) {
    let t = open_thread(pool).await;
    let ana_comment = comment(&t.f.pool, t.id, t.f.ana.id, "Ana's take", 1).await;
    let cy_comment = comment(&t.f.pool, t.id, t.cy.id, "Cy's take", 2).await;
    let dee = t.f.member("dee_s", "public").await;
    mutual(&t.f.pool, dee.id, t.bo.id).await;
    thread(&t.f.pool, dee.id, t.episodes[1]).await;
    let (ana_id, cy_id) = (ana_comment.to_string(), cy_comment.to_string());

    for (user, id) in [(&t.cy, &ana_id), (&t.f.ana, &cy_id), (&dee, &ana_id)] {
        assert_eq!(t.delete(user, id).await, StatusCode::NOT_FOUND);
    }
    for id in [Uuid::new_v4().to_string(), "not-a-uuid".to_owned()] {
        assert_eq!(t.delete(&t.bo, &id).await, StatusCode::NOT_FOUND);
    }
    assert_eq!(t.stored(ana_comment).await.0.as_deref(), Some("Ana's take"));
    assert_eq!(t.stored(cy_comment).await.0.as_deref(), Some("Cy's take"));
    assert_csrf_required(
        &t.f.app,
        &t.bo,
        Method::DELETE,
        &format!("/comments/{cy_id}"),
        "{}",
    )
    .await;

    assert_eq!(t.delete(&t.bo, &cy_id).await, StatusCode::NO_CONTENT);
    for reader in [&t.f.ana, &t.cy, &t.bo] {
        let items = t.read(reader).await;
        let tombstone = find(&items, cy_comment).expect("tombstone stays listed");
        assert!(tombstone["body"].is_null());
        assert_eq!(tombstone["removed"], true);
        assert_eq!(tombstone["canDelete"], false);
        assert!(!json!(items).to_string().contains("Cy's take"));
    }
    let (body, removed_at, removed_by) = t.stored(cy_comment).await;
    assert_eq!((body, removed_by), (None, Some(t.bo.id)));
    assert_eq!(t.delete(&t.bo, &cy_id).await, StatusCode::NO_CONTENT);
    assert_eq!(t.delete(&t.cy, &cy_id).await, StatusCode::NO_CONTENT);
    assert_eq!(
        t.stored(cy_comment).await,
        (None, removed_at, Some(t.bo.id))
    );

    // Authors keep the right to remove their own words after losing access to the thread.
    sqlx::query("DELETE FROM follows WHERE follower_id = $1")
        .bind(t.f.ana.id)
        .execute(&t.f.pool)
        .await
        .unwrap();
    assert_eq!(t.delete(&t.f.ana, &ana_id).await, StatusCode::NO_CONTENT);
    assert_eq!(t.stored(ana_comment).await.2, Some(t.f.ana.id));

    // Operators moderate any thread.
    let dee_comment = comment(&t.f.pool, t.id, dee.id, "Dee's take", 3).await;
    let operator = TestUser::create(&t.f.test.security, true, "operator").await;
    profile(&t.f.pool, operator.id, "op_x", "Op", "private").await;
    let dee_id = dee_comment.to_string();
    assert_eq!(t.delete(&operator, &dee_id).await, StatusCode::NO_CONTENT);
    assert_eq!(t.stored(dee_comment).await.2, Some(operator.id));
    assert_eq!(t.comments().await, 3);
}

/// Acceptance: the reporter stops receiving the comment at once; other readers still see it,
/// and repeating returns the reporter's open report.
#[sqlx::test]
async fn reports_hide_the_comment_from_the_reporter_only(pool: PgPool) {
    let t = open_thread(pool).await;
    let spoiler = comment(&t.f.pool, t.id, t.bo.id, "Bo's spoiler", 1).await;
    let id = spoiler.to_string();

    let (status, first) = t
        .report(&t.f.ana, &id, json!({ "reason": "later_episode" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first.as_object().unwrap().len(), 1);
    let report_id = first["reportId"].as_str().unwrap();
    assert!(find(&t.read(&t.f.ana).await, spoiler).is_none());
    for reader in [&t.cy, &t.bo] {
        let items = t.read(reader).await;
        assert_eq!(find(&items, spoiler).unwrap()["body"], "Bo's spoiler");
    }

    let (status, repeat) = t.report(&t.f.ana, &id, json!({ "reason": "spam" })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(repeat["reportId"], report_id);
    let (status, other) = t.report(&t.cy, &id, json!({ "reason": "abuse" })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(other["reportId"], report_id);
    let reports: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT reporter_id, reason, state FROM reports ORDER BY created_at")
            .fetch_all(&t.f.pool)
            .await
            .unwrap();
    assert_eq!(
        reports,
        [
            (t.f.ana.id, "later_episode".to_owned(), "open".to_owned()),
            (t.cy.id, "abuse".to_owned(), "open".to_owned())
        ]
    );
    assert_eq!(t.stored(spoiler).await.0.as_deref(), Some("Bo's spoiler"));
}

#[sqlx::test]
async fn concurrent_reports_by_one_viewer_create_one_report(pool: PgPool) {
    let t = open_thread(pool).await;
    let id = comment(&t.f.pool, t.id, t.bo.id, "Bo's take", 1)
        .await
        .to_string();
    let reason = || json!({ "reason": "other" });
    let (a, b) = tokio::join!(
        t.report(&t.f.ana, &id, reason()),
        t.report(&t.f.ana, &id, reason())
    );
    assert_eq!((a.0, b.0), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(a.1["reportId"], b.1["reportId"]);
    assert_eq!(count(&t.f.pool, "SELECT count(*) FROM reports").await, 1);
}

/// Reports need an approved reason and a comment the reporter can read now: the thread's privacy
/// and spoiler gates, then a live comment that is not hidden from them or by a blocked author.
#[sqlx::test]
async fn reports_need_a_readable_live_comment_and_an_approved_reason(pool: PgPool) {
    let t = open_thread(pool).await;
    let live = comment(&t.f.pool, t.id, t.bo.id, "Bo's take", 1)
        .await
        .to_string();
    let reason = || json!({ "reason": "abuse" });
    let invalid = |response| {
        assert_code(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
    };
    let concealed = |response| assert_code(response, StatusCode::NOT_FOUND, "NOT_FOUND");

    let (status, body) = t
        .report(&t.f.ana, &live, json!({ "reason": "boring" }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["reason"].is_string());
    assert!(!body.to_string().contains("boring"));
    invalid(t.report(&t.f.ana, &live, json!({})).await);
    invalid(
        t.report(&t.f.ana, &live, json!({ "reason": "spam", "note": "x" }))
            .await,
    );

    // Removed, hidden and blocked-author comments cannot be reported.
    let removed = comment(&t.f.pool, t.id, t.cy.id, "Gone", 2).await;
    sqlx::query("UPDATE comments SET body = NULL, removed_at = now() WHERE id = $1")
        .bind(removed)
        .execute(&t.f.pool)
        .await
        .unwrap();
    let hidden = comment(&t.f.pool, t.id, t.cy.id, "Hidden", 3).await;
    sqlx::query("INSERT INTO hidden_comments (user_id, comment_id) VALUES ($1, $2)")
        .bind(t.f.ana.id)
        .bind(hidden)
        .execute(&t.f.pool)
        .await
        .unwrap();
    let dee = t.f.member("dee_s", "public").await;
    mutual(&t.f.pool, dee.id, t.bo.id).await;
    let blocked = comment(&t.f.pool, t.id, dee.id, "Dee's take", 4).await;
    block(&t.f.pool, dee.id, t.f.ana.id).await;
    for id in [removed, hidden, blocked, Uuid::new_v4()] {
        concealed(t.report(&t.f.ana, &id.to_string(), reason()).await);
    }
    concealed(t.report(&t.f.ana, "not-a-uuid", reason()).await);

    // The thread's gates: unwatched is locked, a stranger is concealed.
    sqlx::query("DELETE FROM episode_progress WHERE user_id = $1")
        .bind(t.cy.id)
        .execute(&t.f.pool)
        .await
        .unwrap();
    assert_code(
        t.report(&t.cy, &live, reason()).await,
        StatusCode::FORBIDDEN,
        "SPOILER_LOCKED",
    );
    let eve = t.f.member("eve_q", "public").await;
    concealed(t.report(&eve, &live, reason()).await);
    assert_eq!(count(&t.f.pool, "SELECT count(*) FROM reports").await, 0);

    // The participant block does not stop Ana reporting others in the thread.
    let (status, _) = t.report(&t.f.ana, &live, reason()).await;
    assert_eq!(status, StatusCode::CREATED);
}
