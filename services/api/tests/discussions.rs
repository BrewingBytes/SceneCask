//! R30 hosted discussion evidence (C07, C09 cases 1, 3 and 7) against migrated PostgreSQL: the
//! one-way/mutual membership matrix, the watched-through gate with gaps, undated predecessors and
//! specials, per-thread reveal grants, revocation by unfollow, block or disabled host after a
//! reveal, participant block filtering, tombstones, hidden and reported comments, response shapes
//! without protected text, cursors and concurrent creates.

use axum::http::{Method, StatusCode};
use scenecask_api::modules::discussions;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, Signed, TestUser, approve, assert_csrf_required, assert_error, block,
    checked, comment, count, disable, mark, mutual, released, seed_show, send, social_fixture,
    thread,
};

async fn fixture(pool: PgPool) -> MemberFixture {
    social_fixture(pool, discussions::routes).await
}

/// A three-episode season plus one special; returns S1E1, S1E2, S1E3 and S0E1.
async fn show(pool: &PgPool) -> [Uuid; 4] {
    let (_, ids) = seed_show(
        pool,
        "Hollow Orchard",
        "returning",
        &[
            released(1, 1),
            released(1, 2),
            released(1, 3),
            released(0, 1),
        ],
    )
    .await;
    ids.try_into().unwrap()
}

/// Asserts a payload carries none of the seeded episode's protected title, overview or still.
fn assert_unprotected(body: &Value) {
    let text = body.to_string();
    for sentinel in ["Protected", "protected-episode"] {
        assert!(!text.contains(sentinel), "leaked {sentinel} in {text}");
    }
}

impl MemberFixture {
    async fn read(&self, user: &TestUser, path: &str) -> (StatusCode, Value) {
        let (status, body) = checked(&self.app, Call::get(path).signed(&user.signed)).await;
        assert_unprotected(&body);
        (status, body)
    }

    async fn threads(&self, user: &TestUser, episode: Uuid) -> Vec<Value> {
        let (status, body) = self
            .read(user, &format!("/episodes/{episode}/discussions"))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["nextCursor"].is_null());
        body["items"].as_array().unwrap().clone()
    }

    async fn comments(&self, user: &TestUser, discussion: Uuid) -> (StatusCode, Value) {
        self.read(user, &format!("/discussions/{discussion}/comments"))
            .await
    }

    /// The comment page `user` reads in `discussion`, asserting the gates let them in.
    async fn unlocked(&self, user: &TestUser, discussion: Uuid) -> Value {
        let (status, page) = self.comments(user, discussion).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    async fn create(&self, user: &TestUser, episode: Uuid) -> (StatusCode, Value) {
        let path = format!("/episodes/{episode}/discussions");
        checked(&self.app, Call::post(&path, "{}").signed(&user.signed)).await
    }

    async fn reveal(&self, user: &TestUser, discussion: Uuid) {
        let body = json!({"scope": "discussion", "resourceId": discussion}).to_string();
        let (status, _) = checked(
            &self.app,
            Call::post("/reveals", &body).signed(&user.signed),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

fn access(item: &Value) -> &str {
    item["access"].as_str().unwrap()
}

fn assert_locked(response: (StatusCode, Value)) {
    let (status, body) = response;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "SPOILER_LOCKED");
    assert!(body.get("items").is_none());
}

fn assert_concealed(response: (StatusCode, Value)) {
    let (status, body) = response;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "NOT_FOUND");
}

fn bodies(page: &Value) -> Vec<Value> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["body"].clone())
        .collect()
}

/// Acceptance: one-way host follow lists nothing and conceals the thread; mutual lists it but
/// the watched-through gate still applies until every predecessor and the target are marked.
#[sqlx::test]
async fn one_way_follow_hides_the_thread_and_mutual_follow_is_still_gated(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let [e1, e2, e3, _] = show(&f.pool).await;
    let id = thread(&f.pool, bo.id, e3).await;
    comment(&f.pool, id, bo.id, "That ending surprised me.", 1).await;

    // Public host and both one-way directions: no membership.
    assert!(f.threads(&f.ana, e3).await.is_empty());
    assert_concealed(f.comments(&f.ana, id).await);
    approve(&f.pool, f.ana.id, bo.id).await;
    assert!(f.threads(&f.ana, e3).await.is_empty());
    assert_concealed(f.comments(&f.ana, id).await);
    sqlx::query("DELETE FROM follows")
        .execute(&f.pool)
        .await
        .unwrap();
    approve(&f.pool, bo.id, f.ana.id).await;
    assert!(f.threads(&f.ana, e3).await.is_empty());
    // A pending reverse edge is not mutual either.
    sqlx::query("INSERT INTO follows (follower_id, followee_id, state) VALUES ($1, $2, 'pending')")
        .bind(f.ana.id)
        .bind(bo.id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(f.threads(&f.ana, e3).await.is_empty());

    approve(&f.pool, f.ana.id, bo.id).await;
    let items = f.threads(&f.ana, e3).await;
    assert_eq!(items.len(), 1);
    let item = &items[0];
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
            "access",
            "host",
            "id",
            "isHost",
            "isSpecial",
            "missingEpisodeCodes"
        ]
    );
    assert_eq!(item["id"], json!(id));
    assert_eq!(item["isHost"], false);
    assert_eq!(item["isSpecial"], false);
    assert_eq!(access(item), "locked");
    assert_eq!(
        item["missingEpisodeCodes"],
        json!(["S1 E1", "S1 E2", "S1 E3"])
    );
    assert_eq!(item["host"]["handle"], "bo_i");
    assert_eq!(item["host"]["relationship"]["mutual"], true);
    // A locked item carries no thread content.
    assert!(!item.to_string().contains("surprised"));
    assert_locked(f.comments(&f.ana, id).await);

    // C09 case 1: the target watched with an E2 gap stays locked.
    mark(&f.pool, f.ana.id, &[e1, e3]).await;
    let item = &f.threads(&f.ana, e3).await[0];
    assert_eq!(access(item), "locked");
    assert_eq!(item["missingEpisodeCodes"], json!(["S1 E2"]));
    assert_locked(f.comments(&f.ana, id).await);

    mark(&f.pool, f.ana.id, &[e2]).await;
    let item = &f.threads(&f.ana, e3).await[0];
    assert_eq!(access(item), "watched");
    assert_eq!(item["missingEpisodeCodes"], json!([]));
    let page = f.unlocked(&f.ana, id).await;
    assert_eq!(bodies(&page), [json!("That ending surprised me.")]);

    // The host's own progress never opens the viewer's gate, and vice versa.
    assert_locked(f.comments(&bo, id).await);
}

/// Acceptance: a grant for thread A on an episode never opens thread B on the same episode.
#[sqlx::test]
async fn reveal_grants_open_only_their_own_thread(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    let cy = f.member("cy_t", "private").await;
    mutual(&f.pool, f.ana.id, bo.id).await;
    mutual(&f.pool, f.ana.id, cy.id).await;
    let [_, e2, _, _] = show(&f.pool).await;
    let a = thread(&f.pool, bo.id, e2).await;
    let b = thread(&f.pool, cy.id, e2).await;
    comment(&f.pool, a, bo.id, "Thread A text", 1).await;
    comment(&f.pool, b, cy.id, "Thread B text", 1).await;

    f.reveal(&f.ana, a).await;
    let items = f.threads(&f.ana, e2).await;
    let state = |id: Uuid| {
        items
            .iter()
            .find(|item| item["id"] == json!(id))
            .map(|item| access(item).to_owned())
    };
    assert_eq!(state(a).as_deref(), Some("revealed"));
    assert_eq!(state(b).as_deref(), Some("locked"));
    // A reveal marks nothing, so the missing codes remain.
    let a_item = items.iter().find(|item| item["id"] == json!(a)).unwrap();
    assert_eq!(a_item["missingEpisodeCodes"], json!(["S1 E1", "S1 E2"]));
    let page = f.unlocked(&f.ana, a).await;
    assert_eq!(bodies(&page), [json!("Thread A text")]);
    assert_locked(f.comments(&f.ana, b).await);
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM episode_progress").await,
        0
    );

    // Another session of the same viewer does not inherit the grant.
    let other = common::sign_in(&f.test.security, f.ana.id).await;
    let (status, _, body) = send(
        &f.app,
        Call::get(&format!("/discussions/{a}/comments")).signed(&other),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Hiding again locks the thread on the next read.
    let path = format!("/reveals/discussion/{a}");
    let (status, _) = checked(
        &f.app,
        Call::write(Method::DELETE, &path, "{}").signed(&f.ana.signed),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_locked(f.comments(&f.ana, a).await);
}

/// C09 case 7: unfollow, block or a disabled host revokes access even after a reveal, and a
/// restored mutual follow does not bring a revoked grant back.
#[sqlx::test]
async fn revoked_membership_supersedes_a_previous_reveal(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    mutual(&f.pool, f.ana.id, bo.id).await;
    let [e1, _, _, _] = show(&f.pool).await;
    let id = thread(&f.pool, bo.id, e1).await;
    f.reveal(&f.ana, id).await;
    f.unlocked(&f.ana, id).await;

    // Unfollow without the social module's grant cleanup: the stale grant is still refused.
    sqlx::query("DELETE FROM follows WHERE follower_id = $1")
        .bind(f.ana.id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM reveal_grants").await,
        1
    );
    assert!(f.threads(&f.ana, e1).await.is_empty());
    assert_concealed(f.comments(&f.ana, id).await);

    approve(&f.pool, f.ana.id, bo.id).await;
    f.unlocked(&f.ana, id).await;

    for blocker in [f.ana.id, bo.id] {
        let other = if blocker == bo.id { f.ana.id } else { bo.id };
        block(&f.pool, blocker, other).await;
        assert!(f.threads(&f.ana, e1).await.is_empty());
        assert_concealed(f.comments(&f.ana, id).await);
        sqlx::query("DELETE FROM blocks")
            .execute(&f.pool)
            .await
            .unwrap();
    }

    disable(&f.pool, bo.id).await;
    assert!(f.threads(&f.ana, e1).await.is_empty());
    assert_concealed(f.comments(&f.ana, id).await);
}

/// Specials always need an explicit reveal, and an undated unmarked predecessor gates the thread.
#[sqlx::test]
async fn specials_need_a_reveal_and_undated_gaps_stay_locked(pool: PgPool) {
    let f = fixture(pool).await;
    let (_, ids) = seed_show(
        &f.pool,
        "Undated show",
        "returning",
        &[released(1, 1), (1, 2, None), released(1, 3), released(0, 1)],
    )
    .await;
    let (e1, e2, e3, special) = (ids[0], ids[1], ids[2], ids[3]);
    mark(&f.pool, f.ana.id, &[e1, e3, special]).await;

    let regular = thread(&f.pool, f.ana.id, e3).await;
    let item = &f.threads(&f.ana, e3).await[0];
    assert_eq!(item["isHost"], true);
    assert_eq!(access(item), "locked");
    assert_eq!(item["missingEpisodeCodes"], json!(["S1 E2"]));
    assert_locked(f.comments(&f.ana, regular).await);
    mark(&f.pool, f.ana.id, &[e2]).await;
    f.unlocked(&f.ana, regular).await;

    let extra = thread(&f.pool, f.ana.id, special).await;
    let item = &f.threads(&f.ana, special).await[0];
    assert_eq!(item["isSpecial"], true);
    assert_eq!(access(item), "locked");
    assert_eq!(item["missingEpisodeCodes"], json!([]));
    assert_locked(f.comments(&f.ana, extra).await);
    f.reveal(&f.ana, extra).await;
    assert_eq!(access(&f.threads(&f.ana, special).await[0]), "revealed");
    f.unlocked(&f.ana, extra).await;
}

/// Acceptance: a participant block hides that participant's comments and identity both ways but
/// keeps host access; tombstones, hidden and reported comments follow C07.
#[sqlx::test]
async fn participant_blocks_hide_comments_and_identities_but_keep_host_access(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    let cy = f.member("cy_t", "private").await;
    let dee = f.member("dee_v", "private").await;
    for participant in [f.ana.id, cy.id, dee.id] {
        mutual(&f.pool, participant, bo.id).await;
    }
    let [e1, _, _, _] = show(&f.pool).await;
    for reader in [f.ana.id, bo.id, cy.id] {
        mark(&f.pool, reader, &[e1]).await;
    }
    let id = thread(&f.pool, bo.id, e1).await;
    let host_comment = comment(&f.pool, id, bo.id, "Host opener", 1).await;
    comment(&f.pool, id, cy.id, "Cy reply", 2).await;
    let own = comment(&f.pool, id, f.ana.id, "Ana reply", 3).await;
    let removed = comment(&f.pool, id, dee.id, "Removed original text", 4).await;
    let hidden = comment(&f.pool, id, dee.id, "Hidden by Ana", 5).await;
    let reported = comment(&f.pool, id, dee.id, "Reported by Ana", 6).await;
    sqlx::query(
        "UPDATE comments SET body = NULL, removed_at = now(), removed_by = author_id WHERE id = $1",
    )
    .bind(removed)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO hidden_comments (user_id, comment_id) VALUES ($1, $2)")
        .bind(f.ana.id)
        .bind(hidden)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO reports (reporter_id, comment_id, reason) VALUES ($1, $2, 'spam')")
        .bind(f.ana.id)
        .bind(reported)
        .execute(&f.pool)
        .await
        .unwrap();

    let page = f.unlocked(&f.ana, id).await;
    let all = ["Host opener", "Cy reply", "Ana reply"];
    assert_eq!(
        bodies(&page),
        [&all.map(|b| json!(b))[..], &[Value::Null]].concat()
    );
    let items = page["items"].as_array().unwrap();
    // Participants see one another without following each other.
    assert_eq!(items[1]["author"]["handle"], "cy_t");
    assert_eq!(items[1]["author"]["relationship"]["mutual"], false);
    assert_eq!(items[0]["id"], json!(host_comment));
    assert_eq!(items[0]["canDelete"], false);
    assert_eq!(items[2]["id"], json!(own));
    assert_eq!(items[2]["canDelete"], true);
    assert_eq!(items[0]["canModerate"], false);
    let tombstone = &items[3];
    assert_eq!(
        *tombstone,
        json!({
            "id": removed, "author": tombstone["author"].clone(), "body": null,
            "createdAt": "2026-10-01T00:00:04.000Z", "removed": true,
            "canDelete": false, "canModerate": false
        })
    );
    assert!(!page.to_string().contains("Removed original"));
    // Other participants still see what Ana hid or reported.
    let (_, cy_page) = f.comments(&cy, id).await;
    assert_eq!(cy_page["items"].as_array().unwrap().len(), 6);

    // Either block direction drops the comments and identity, not Ana's access.
    for (blocker, blocked) in [(f.ana.id, cy.id), (cy.id, f.ana.id)] {
        block(&f.pool, blocker, blocked).await;
        let page = f.unlocked(&f.ana, id).await;
        assert_eq!(bodies(&page), [json!(all[0]), json!(all[2]), Value::Null]);
        assert!(!page.to_string().contains("cy_t"));
        assert_eq!(f.threads(&f.ana, e1).await.len(), 1);
        let (_, cy_page) = f.comments(&cy, id).await;
        assert!(!cy_page.to_string().contains("Ana reply"));
        assert!(!cy_page.to_string().contains("ana_r"));
        sqlx::query("DELETE FROM blocks")
            .execute(&f.pool)
            .await
            .unwrap();
    }

    // The host may delete every live comment in their own thread.
    let (_, host_page) = f.comments(&bo, id).await;
    let deletable: Vec<bool> = host_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["canDelete"].as_bool().unwrap())
        .collect();
    assert_eq!(deletable, [true, true, true, false, true, true]);

    // A disabled author keeps the comment but loses the identity.
    disable(&f.pool, cy.id).await;
    let (_, page) = f.comments(&f.ana, id).await;
    assert!(page["items"][1]["author"].is_null());
    assert_eq!(page["items"][1]["body"], "Cy reply");
}

#[sqlx::test]
async fn comments_and_threads_page_by_cursor(pool: PgPool) {
    let f = fixture(pool).await;
    let [e1, _, _, _] = show(&f.pool).await;
    mark(&f.pool, f.ana.id, &[e1]).await;
    let id = thread(&f.pool, f.ana.id, e1).await;
    for n in 0..5 {
        comment(&f.pool, id, f.ana.id, &format!("Comment {n}"), n).await;
    }
    let mut seen = Vec::new();
    let mut path = format!("/discussions/{id}/comments?limit=2");
    loop {
        let (status, page) = f.read(&f.ana, &path).await;
        assert_eq!(status, StatusCode::OK);
        seen.extend(bodies(&page));
        match page["nextCursor"].as_str() {
            Some(cursor) => path = format!("/discussions/{id}/comments?limit=2&cursor={cursor}"),
            None => break,
        }
    }
    let expected: Vec<Value> = (0..5).map(|n| json!(format!("Comment {n}"))).collect();
    assert_eq!(seen, expected);

    // Threads page newest first.
    let mut hosts = Vec::new();
    for handle in ["bo_i", "cy_t", "dee_v"] {
        let host = f.member(handle, "private").await;
        mutual(&f.pool, f.ana.id, host.id).await;
        hosts.push(thread(&f.pool, host.id, e1).await);
    }
    let mut listed = Vec::new();
    let mut path = format!("/episodes/{e1}/discussions?limit=3");
    loop {
        let (status, page) = f.read(&f.ana, &path).await;
        assert_eq!(status, StatusCode::OK);
        listed.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["id"].clone()),
        );
        match page["nextCursor"].as_str() {
            Some(cursor) => path = format!("/episodes/{e1}/discussions?limit=3&cursor={cursor}"),
            None => break,
        }
    }
    let mut expected: Vec<Value> = std::iter::once(id)
        .chain(hosts)
        .map(|id| json!(id))
        .collect();
    expected.reverse();
    assert_eq!(listed, expected);

    for (path, code, status) in [
        (
            format!("/discussions/{id}/comments?cursor=bad"),
            "VALIDATION_ERROR",
            400,
        ),
        (
            format!("/discussions/{id}/comments?limit=51"),
            "VALIDATION_ERROR",
            422,
        ),
        (
            format!("/episodes/{e1}/discussions?cursor=%25"),
            "VALIDATION_ERROR",
            400,
        ),
        (
            format!("/episodes/{e1}/discussions?limit=0"),
            "VALIDATION_ERROR",
            422,
        ),
        (
            format!("/discussions/{}/comments", Uuid::new_v4()),
            "NOT_FOUND",
            404,
        ),
        (
            "/discussions/not-a-uuid/comments".to_owned(),
            "NOT_FOUND",
            404,
        ),
        (
            format!("/episodes/{}/discussions", Uuid::new_v4()),
            "NOT_FOUND",
            404,
        ),
    ] {
        let (actual, headers, body) = send(&f.app, Call::get(&path).signed(&f.ana.signed)).await;
        assert_error(actual, &headers, &body, status, code);
    }
}

#[sqlx::test]
async fn create_is_idempotent_per_host_and_episode_and_race_safe(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "private").await;
    let [e1, e2, _, _] = show(&f.pool).await;

    // Progress is not required to create, and the host's own thread starts locked.
    let (status, created) = f.create(&f.ana, e2).await;
    assert_eq!(status, StatusCode::CREATED);
    let mut keys: Vec<&str> = created
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["access", "id"]);
    assert_eq!(created["access"], "locked");
    let (status, again) = f.create(&f.ana, e2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["id"], created["id"]);

    mark(&f.pool, f.ana.id, &[e1]).await;
    let (status, body) = f.create(&f.ana, e1).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["access"], "watched");

    // Concurrent creates converge on one thread.
    let calls = (0..6).map(|_| {
        let app = f.app.clone();
        let signed = Signed {
            cookie: bo.signed.cookie.clone(),
            csrf: bo.signed.csrf.clone(),
            hash: bo.signed.hash.clone(),
        };
        let path = format!("/episodes/{e2}/discussions");
        tokio::spawn(async move {
            let (status, _, body) = send(&app, Call::post(&path, "{}").signed(&signed)).await;
            (status, body["id"].clone())
        })
    });
    let results: Vec<(StatusCode, Value)> = futures_join(calls).await;
    assert_eq!(
        results
            .iter()
            .filter(|(status, _)| *status == StatusCode::CREATED)
            .count(),
        1
    );
    assert!(
        results
            .iter()
            .all(|(status, id)| status.is_success() && *id == results[0].1)
    );
    assert_eq!(count(&f.pool, "SELECT count(*) FROM discussions").await, 3);

    // Unknown, malformed and unprotected writes create nothing.
    let unknown = format!("/episodes/{}/discussions", Uuid::new_v4());
    let (status, headers, body) =
        send(&f.app, Call::post(&unknown, "{}").signed(&f.ana.signed)).await;
    assert_error(status, &headers, &body, 404, "NOT_FOUND");
    let path = format!("/episodes/{e1}/discussions");
    let (status, _) = checked(
        &f.app,
        Call::post(&path, r#"{"title":"x"}"#).signed(&bo.signed),
    )
    .await;
    assert!(status.is_client_error());
    assert_csrf_required(&f.app, &bo, Method::POST, &path, "{}").await;
    assert_eq!(count(&f.pool, "SELECT count(*) FROM discussions").await, 3);

    // Before onboarding there is no host card to list, so hosting needs a handle first.
    let fresh = TestUser::create(&f.test.security, true, "member").await;
    let (status, headers, body) = send(&f.app, Call::post(&path, "{}").signed(&fresh.signed)).await;
    assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
    assert!(body["error"]["fields"]["handle"].is_string());
    assert_eq!(count(&f.pool, "SELECT count(*) FROM discussions").await, 3);
}

async fn futures_join<T: Send + 'static>(
    handles: impl Iterator<Item = tokio::task::JoinHandle<T>>,
) -> Vec<T> {
    let mut results = Vec::new();
    for handle in handles.collect::<Vec<_>>() {
        results.push(handle.await.unwrap());
    }
    results
}
