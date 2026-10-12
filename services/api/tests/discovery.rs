//! R27 social discovery evidence (C06) against migrated PostgreSQL: library saves, watched marks
//! and catch-up project deduplicated activity holding episode codes only; replays and no-ops add
//! nothing and Undo removes reversed events. The feed and recommendations recheck follows, blocks
//! and account state on every read, and recommendations rank followed people's saves by count
//! with a show ID tie-break under a snapshot cursor. Without follows both are empty.

use axum::http::{Method, StatusCode};
use scenecask_api::modules::{library, social, tracking};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{
    Call, MemberFixture, TestUser, assert_error, checked, count, member_fixture, profile, released,
    seed_show, send,
};

async fn fixture(pool: PgPool) -> MemberFixture {
    let f = member_fixture(pool, |security| {
        social::routes(security.clone())
            .merge(tracking::routes(security.clone()))
            .merge(tracking::catchup::routes(security.clone()))
            .merge(library::routes(security.clone()))
    })
    .await;
    profile(&f.pool, f.ana.id, "ana_r", "Ana Rusu", "private").await;
    f
}

/// A four-episode returning show, all released.
async fn four_episodes(pool: &PgPool, title: &str) -> (Uuid, Vec<Uuid>) {
    let episodes = [
        released(1, 1),
        released(1, 2),
        released(1, 3),
        released(2, 1),
    ];
    seed_show(pool, title, "returning", &episodes).await
}

/// Ana with an approved follow of public Bo, and a four-episode show.
async fn followed(pool: PgPool) -> (MemberFixture, TestUser, Uuid, Vec<Uuid>) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    f.follows(&f.ana, bo.id, "approved").await;
    let (show, eps) = four_episodes(&f.pool, "Hollow Orchard").await;
    (f, bo, show, eps)
}

/// Saves `show` for `user` directly, as an entry predating this test's reads.
async fn saved(pool: &PgPool, user: Uuid, show: Uuid) {
    sqlx::query(
        "INSERT INTO library_entries (user_id, show_id, saved, status, saved_at, revision)
         VALUES ($1, $2, true, 'plan_to_watch', clock_timestamp(), 1)",
    )
    .bind(user)
    .bind(show)
    .execute(pool)
    .await
    .unwrap();
}

/// The activity rows, as (kind, episode) in creation order.
async fn events(pool: &PgPool) -> Vec<(String, Option<Uuid>)> {
    sqlx::query_as("SELECT kind, episode_id FROM activity_events ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

impl MemberFixture {
    /// `user`'s keyed write with `key`; returns the status and body.
    async fn keyed(
        &self,
        user: &TestUser,
        key: &str,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let body = body.to_string();
        let call = Call::write(method, path, &body)
            .idempotent(key)
            .signed(&user.signed);
        let (status, _, body) = send(&self.app, call).await;
        (status, body)
    }

    /// `user`'s keyed write with a fresh key, expecting 200.
    async fn ok(&self, user: &TestUser, method: Method, path: &str, body: Value) -> Value {
        let key = Uuid::new_v4().to_string();
        let (status, body) = self.keyed(user, &key, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn save(&self, user: &TestUser, show: Uuid, expected_revision: i64) -> Value {
        let body = json!({"saved": true, "expectedRevision": expected_revision});
        self.ok(user, Method::PUT, &format!("/library/{show}"), body)
            .await
    }

    async fn mark(&self, user: &TestUser, episode: Uuid, watched: bool, revision: i64) -> Value {
        let path = format!("/progress/episodes/{episode}");
        self.ok(
            user,
            Method::PUT,
            &path,
            json!({"watched": watched, "expectedRevision": revision}),
        )
        .await
    }

    async fn undo(&self, user: &TestUser, action: &Value) -> Value {
        let path = format!("/actions/{}/undo", action.as_str().unwrap());
        self.ok(user, Method::POST, &path, json!({})).await
    }

    /// `user` catches up through `through` and returns the MutationResult.
    async fn catch_up(&self, user: &TestUser, show: Uuid, through: Uuid) -> Value {
        let path = format!("/shows/{show}/catch-up/preview");
        let body = json!({"throughEpisodeId": through}).to_string();
        let (status, _, preview) =
            send(&self.app, Call::post(&path, &body).signed(&user.signed)).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let body = json!({"previewId": preview["previewId"]});
        self.ok(user, Method::POST, &format!("/shows/{show}/catch-up"), body)
            .await
    }

    /// `user` follows `target`, expecting `state`.
    async fn follows(&self, user: &TestUser, target: Uuid, state: &str) {
        let path = format!("/people/{target}/follow");
        let body = json!({}).to_string();
        let call = Call::post(&path, &body).signed(&user.signed);
        let (status, body) = checked(&self.app, call).await;
        assert_eq!((status, &body["state"]), (StatusCode::OK, &json!(state)));
    }

    /// An unkeyed social write that answers 204.
    async fn social(&self, user: &TestUser, method: Method, path: &str, body: Value) {
        let body = body.to_string();
        let call = Call::write(method, path, &body).signed(&user.signed);
        assert_eq!(checked(&self.app, call).await.0, StatusCode::NO_CONTENT);
    }

    /// `user`'s page at `path`, expecting 200, with the raw JSON text for redaction checks.
    async fn page(&self, user: &TestUser, path: &str) -> Value {
        let (status, body) = checked(&self.app, Call::get(path).signed(&user.signed)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let text = body.to_string();
        for protected in ["Protected", "protected-episode", "@example.test"] {
            assert!(!text.contains(protected), "{protected} leaked: {text}");
        }
        body
    }

    async fn activity(&self, user: &TestUser) -> Vec<Value> {
        let body = self.page(user, "/activity").await;
        assert_eq!(body["nextCursor"], Value::Null);
        body["items"].as_array().unwrap().clone()
    }

    /// Recommended show IDs with their people's handles.
    async fn recommended(&self, user: &TestUser, path: &str) -> (Vec<(Value, Vec<Value>)>, Value) {
        let body = self.page(user, path).await;
        let items = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                assert_eq!(item["reason"]["kind"], "followed_people");
                let people = item["reason"]["people"].as_array().unwrap();
                let handles = people.iter().map(|p| p["handle"].clone()).collect();
                (item["show"]["id"].clone(), handles)
            })
            .collect();
        (items, body["nextCursor"].clone())
    }
}

#[sqlx::test]
async fn without_follows_activity_and_recommendations_are_empty(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let (show, eps) = four_episodes(&f.pool, "Hollow Orchard").await;
    f.save(&bo, show, 0).await;
    f.mark(&bo, eps[0], true, 0).await;
    // Ana's own tracking works as before and stays out of her feed.
    let (own, own_eps) = four_episodes(&f.pool, "Quiet Harbor").await;
    let marked = f.mark(&f.ana, own_eps[0], true, 0).await;
    assert_eq!(marked["changed"], 1);
    assert_eq!(marked["library"]["saved"], true);
    assert_eq!(f.save(&f.ana, own, 1).await["changed"], 0);

    for path in ["/activity", "/recommendations"] {
        assert_eq!(
            f.page(&f.ana, path).await,
            json!({"items": [], "nextCursor": null})
        );
    }
    // A public account alone is not a follow: the stranger's feed is empty too.
    assert!(
        f.activity(&f.member("cy_n", "private").await)
            .await
            .is_empty()
    );
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM activity_events").await,
        3
    );
}

#[sqlx::test]
async fn saves_and_marks_project_one_event_each_with_only_an_episode_code(pool: PgPool) {
    let (f, bo, show, eps) = followed(pool).await;

    f.save(&bo, show, 0).await;
    // A status change and a repeated save are not new additions.
    let path = format!("/library/{show}");
    f.ok(
        &bo,
        Method::PATCH,
        &path,
        json!({"status": "watching", "expectedRevision": 1}),
    )
    .await;
    assert_eq!(f.save(&bo, show, 2).await["changed"], 0);

    let key = Uuid::new_v4().to_string();
    let path = format!("/progress/episodes/{}", eps[2]);
    let body = json!({"watched": true, "expectedRevision": 0});
    let first = f.keyed(&bo, &key, Method::PUT, &path, body.clone()).await;
    // A retried key replays the stored result; an unchanged value is a no-op. Neither projects.
    let replay = f.keyed(&bo, &key, Method::PUT, &path, body).await;
    assert_eq!(first, replay);
    assert_eq!(f.mark(&bo, eps[2], true, 1).await["changed"], 0);
    assert_eq!(
        events(&f.pool).await,
        [
            ("library_added".into(), None),
            ("episode_watched".into(), Some(eps[2]))
        ]
    );

    let items = f.activity(&f.ana).await;
    assert_eq!(items.len(), 2);
    let watched = items[0].as_object().unwrap();
    let mut keys: Vec<&str> = watched.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["actor", "createdAt", "episode", "id", "kind", "show"]
    );
    assert_eq!(watched["kind"], "episode_watched");
    assert_eq!(
        watched["episode"],
        json!({"id": eps[2], "season": 1, "number": 3})
    );
    assert_eq!(
        watched["show"],
        json!({"id": show, "title": "Hollow Orchard",
               "posterUrl": "https://image.tmdb.org/t/p/w500/poster.jpg"})
    );
    assert_eq!(watched["actor"]["handle"], "bo_i");
    assert_eq!(
        watched["actor"]["relationship"],
        json!({"outgoing": "approved", "incoming": "none", "mutual": false})
    );
    assert_eq!(items[1]["kind"], "library_added");
    assert_eq!(items[1]["episode"], Value::Null);

    // Unwatching keeps the old event; watching again replaces it rather than duplicating it.
    f.mark(&bo, eps[2], false, 1).await;
    f.mark(&bo, eps[2], true, 2).await;
    let items = f.activity(&f.ana).await;
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["id"], watched["id"]);
}

#[sqlx::test]
async fn undo_removes_the_events_of_reversed_fields_only(pool: PgPool) {
    let (f, bo, show, eps) = followed(pool).await;

    // A mark on an unsaved show saves it, but projects only the mark.
    let first = f.mark(&bo, eps[0], true, 0).await;
    let second = f.mark(&bo, eps[1], true, 0).await;
    assert_eq!(
        events(&f.pool).await,
        [
            ("episode_watched".into(), Some(eps[0])),
            ("episode_watched".into(), Some(eps[1]))
        ]
    );
    let undone = f.undo(&bo, &second["actionId"]).await;
    assert_eq!(undone["reverted"], 1);
    assert_eq!(
        events(&f.pool).await,
        [("episode_watched".into(), Some(eps[0]))]
    );
    // Repeating the undo returns the stored outcome and touches nothing.
    assert_eq!(f.undo(&bo, &second["actionId"]).await, undone);

    // A later edit owns E1, so undoing the first mark skips it and keeps its event.
    f.mark(&bo, eps[0], false, 1).await;
    f.mark(&bo, eps[0], true, 2).await;
    let skipped = f.undo(&bo, &first["actionId"]).await;
    assert_eq!(
        (skipped["reverted"].clone(), skipped["skipped"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(events(&f.pool).await.len(), 1);

    // Undoing a save removes its library event.
    let (other, _) = seed_show(&f.pool, "Quiet Harbor", "returning", &[released(1, 1)]).await;
    let save = f.save(&bo, other, 0).await;
    assert_eq!(f.activity(&f.ana).await[0]["kind"], "library_added");
    f.undo(&bo, &save["actionId"]).await;
    let items = f.activity(&f.ana).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["show"]["id"], json!(show));
}

#[sqlx::test]
async fn catch_up_projects_at_most_its_endpoint(pool: PgPool) {
    let (f, bo, show, eps) = followed(pool).await;

    let result = f.catch_up(&bo, show, eps[3]).await;
    assert_eq!(result["changed"], 4);
    assert_eq!(
        events(&f.pool).await,
        [("episode_watched".into(), Some(eps[3]))]
    );
    let items = f.activity(&f.ana).await;
    assert_eq!(
        items[0]["episode"],
        json!({"id": eps[3], "season": 2, "number": 1})
    );

    // An endpoint the catch-up does not mark projects nothing, even when earlier episodes change.
    let (other, more) = four_episodes(&f.pool, "Quiet Harbor").await;
    f.mark(&bo, more[2], true, 0).await;
    assert_eq!(f.catch_up(&bo, other, more[2]).await["changed"], 2);
    assert_eq!(
        events(&f.pool).await.last().unwrap(),
        &("episode_watched".into(), Some(more[2]))
    );
    assert_eq!(events(&f.pool).await.len(), 2);

    f.undo(&bo, &result["actionId"]).await;
    assert_eq!(events(&f.pool).await.len(), 1);
}

#[sqlx::test]
async fn unfollow_removal_and_blocks_hide_old_events_and_unblock_restores_none(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let cy = f.member("cy_n", "private").await;
    f.follows(&f.ana, bo.id, "approved").await;
    f.follows(&f.ana, cy.id, "pending").await;
    let (show, eps) = four_episodes(&f.pool, "Hollow Orchard").await;
    f.mark(&bo, eps[0], true, 0).await;
    f.mark(&cy, eps[0], true, 0).await;
    let (other, _) = four_episodes(&f.pool, "Quiet Harbor").await;
    saved(&f.pool, cy.id, other).await;

    // A pending request sees nothing of the private account.
    let actors = |items: Vec<Value>| -> Vec<Value> {
        items
            .iter()
            .map(|item| item["actor"]["handle"].clone())
            .collect()
    };
    assert_eq!(actors(f.activity(&f.ana).await), ["bo_i"]);
    let path = format!("/me/follow-requests/{}", f.ana.id);
    f.social(&cy, Method::POST, &path, json!({"decision": "approve"}))
        .await;
    assert_eq!(actors(f.activity(&f.ana).await), ["cy_n", "bo_i"]);
    assert_eq!(f.recommended(&f.ana, "/recommendations").await.0.len(), 2);

    // Bo going private keeps Ana's approved follow; a nonfollower never saw the events.
    profile(&f.pool, bo.id, "bo_i", "Bo Ilie", "private").await;
    assert_eq!(actors(f.activity(&f.ana).await), ["cy_n", "bo_i"]);

    // Cy removes Ana as a follower: Cy's old events and saves disappear on the next read.
    f.social(
        &cy,
        Method::DELETE,
        &format!("/me/followers/{}", f.ana.id),
        json!({}),
    )
    .await;
    assert_eq!(actors(f.activity(&f.ana).await), ["bo_i"]);
    let (items, _) = f.recommended(&f.ana, "/recommendations").await;
    assert_eq!(items, [(json!(show), vec![json!("bo_i")])]);

    // Bo blocks Ana: everything of Bo's goes, and unblocking brings back no follow.
    let blocks = format!("/me/blocks/{}", f.ana.id);
    for method in [Method::PUT, Method::DELETE] {
        f.social(&bo, method, &blocks, json!({})).await;
        assert!(f.activity(&f.ana).await.is_empty());
        assert!(f.recommended(&f.ana, "/recommendations").await.0.is_empty());
    }
    // Events stay stored for the actor's other followers.
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM activity_events").await,
        2
    );
}

#[sqlx::test]
async fn a_block_without_edge_changes_or_a_disabled_actor_also_hides_events(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let cy = f.member("cy_n", "public").await;
    f.follows(&f.ana, bo.id, "approved").await;
    f.follows(&f.ana, cy.id, "approved").await;
    let (_, eps) = four_episodes(&f.pool, "Hollow Orchard").await;
    f.mark(&bo, eps[0], true, 0).await;
    f.mark(&cy, eps[0], true, 0).await;
    // A block row whose edges another writer has not removed yet is still enforced at read.
    sqlx::query("INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)")
        .bind(f.ana.id)
        .bind(bo.id)
        .execute(&f.pool)
        .await
        .unwrap();
    common::disable(&f.pool, cy.id).await;
    assert!(f.activity(&f.ana).await.is_empty());
    assert!(f.recommended(&f.ana, "/recommendations").await.0.is_empty());
}

#[sqlx::test]
async fn recommendations_rank_by_followed_people_then_show_id_under_a_snapshot(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    let cy = f.member("cy_n", "public").await;
    let di = f.member("di_a", "private").await;
    let ed = f.member("ed_o", "private").await;
    f.follows(&f.ana, bo.id, "approved").await;
    f.follows(&f.ana, cy.id, "approved").await;
    f.follows(&f.ana, di.id, "pending").await;
    f.social(
        &di,
        Method::POST,
        &format!("/me/follow-requests/{}", f.ana.id),
        json!({"decision": "approve"}),
    )
    .await;
    f.follows(&f.ana, ed.id, "pending").await;

    let mut shows = Vec::new();
    for title in ["One", "Two", "Three", "Four", "Five", "Six"] {
        shows.push(four_episodes(&f.pool, title).await.0);
    }
    // Three people: shows[0]. Two: shows[1]. One each: shows[2] and shows[3], a tie.
    for user in [&cy, &bo, &di] {
        saved(&f.pool, user.id, shows[0]).await;
    }
    saved(&f.pool, bo.id, shows[1]).await;
    saved(&f.pool, di.id, shows[1]).await;
    saved(&f.pool, cy.id, shows[2]).await;
    saved(&f.pool, di.id, shows[3]).await;
    // Ana's own saved show, a pending follow's show and an unsaved entry are left out.
    saved(&f.pool, bo.id, shows[4]).await;
    saved(&f.pool, f.ana.id, shows[4]).await;
    saved(&f.pool, ed.id, shows[5]).await;
    sqlx::query(
        "UPDATE library_entries SET saved = false, revision = revision + 1
             WHERE user_id = $1 AND show_id = $2",
    )
    .bind(cy.id)
    .bind(shows[2])
    .execute(&f.pool)
    .await
    .unwrap();
    saved(&f.pool, bo.id, shows[2]).await;

    let (first, cursor) = f.recommended(&f.ana, "/recommendations?limit=2").await;
    assert_eq!(
        first,
        [
            (
                json!(shows[0]),
                vec![json!("bo_i"), json!("cy_n"), json!("di_a")]
            ),
            (json!(shows[1]), vec![json!("bo_i"), json!("di_a")]),
        ]
    );
    let page = f.page(&f.ana, "/recommendations?limit=1").await;
    let item = &page["items"][0];
    assert_eq!(
        item["show"],
        json!({"id": shows[0], "title": "One", "year": null,
               "posterUrl": "https://image.tmdb.org/t/p/w500/poster.jpg"})
    );
    assert_eq!(item["reason"]["people"][0]["displayName"], "Bo Ilie");

    // A save after the first page cannot reorder the pages that follow it.
    saved(&f.pool, cy.id, shows[3]).await;
    let path = format!(
        "/recommendations?limit=2&cursor={}",
        cursor.as_str().unwrap()
    );
    let (second, end) = f.recommended(&f.ana, &path).await;
    let mut tied = [shows[2], shows[3]];
    tied.sort_unstable();
    let handle = |id: Uuid| json!(if id == shows[2] { "bo_i" } else { "di_a" });
    assert_eq!(
        second,
        tied.iter()
            .map(|&id| (json!(id), vec![handle(id)]))
            .collect::<Vec<_>>()
    );
    assert_eq!(end, Value::Null);

    // A fresh first page sees the new save.
    let (fresh, _) = f.recommended(&f.ana, "/recommendations").await;
    let (_, people) = fresh.iter().find(|(id, _)| *id == json!(shows[3])).unwrap();
    assert_eq!(people, &[json!("cy_n"), json!("di_a")]);
}

#[sqlx::test]
async fn activity_pages_by_cursor_and_rejects_bad_parameters(pool: PgPool) {
    let f = fixture(pool).await;
    let bo = f.member("bo_i", "public").await;
    f.follows(&f.ana, bo.id, "approved").await;
    let (_, eps) = four_episodes(&f.pool, "Hollow Orchard").await;
    for (i, &episode) in eps.iter().enumerate() {
        f.mark(&bo, episode, true, 0).await;
        assert_eq!(
            count(&f.pool, "SELECT count(*) FROM activity_events").await,
            i as i64 + 1
        );
    }
    let mut seen = Vec::new();
    let mut path = "/activity?limit=3".to_owned();
    loop {
        let body = f.page(&f.ana, &path).await;
        seen.extend(
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["episode"]["id"].clone()),
        );
        match body["nextCursor"].as_str() {
            Some(cursor) => path = format!("/activity?limit=3&cursor={cursor}"),
            None => break,
        }
    }
    let newest_first: Vec<Value> = eps.iter().rev().map(|id| json!(id)).collect();
    assert_eq!(seen, newest_first);

    for path in ["/activity", "/recommendations"] {
        let bad_cursor = format!("{path}?cursor=not-a-cursor");
        let call = Call::get(&bad_cursor).signed(&f.ana.signed);
        let (status, headers, body) = send(&f.app, call).await;
        assert_error(status, &headers, &body, 400, "VALIDATION_ERROR");
        let bad_limit = format!("{path}?limit=51");
        let call = Call::get(&bad_limit).signed(&f.ana.signed);
        let (status, headers, body) = send(&f.app, call).await;
        assert_error(status, &headers, &body, 422, "VALIDATION_ERROR");
        let (status, headers, body) = send(&f.app, Call::get(path)).await;
        assert_error(status, &headers, &body, 401, "AUTH_REQUIRED");
    }
}
