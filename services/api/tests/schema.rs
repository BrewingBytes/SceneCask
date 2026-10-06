//! C02 schema tests against real PostgreSQL. `#[sqlx::test]` is the test database factory:
//! each test gets a fresh database (from DATABASE_URL's server) with all migrations applied.
//! Fixtures in `schema_fixtures/` are development/test data only.

use sqlx::{AssertSqlSafe, PgPool, migrate::Migrator};

static MIGRATOR: Migrator = sqlx::migrate!();

const ANA: &str = "00000000-0000-4000-8000-00000000a0a1";
const BEN: &str = "00000000-0000-4000-8000-00000000b0b1";
const CLEO: &str = "00000000-0000-4000-8000-00000000c0c1";

/// Expected C02 tables and columns, in declaration order.
const SCHEMA: &[(&str, &[&str])] = &[
    (
        "users",
        &[
            "id",
            "normalized_email",
            "display_name",
            "handle",
            "visibility",
            "verified_at",
            "role",
            "disabled_at",
            "created_at",
        ],
    ),
    (
        "password_credentials",
        &["user_id", "argon2_hash", "updated_at"],
    ),
    (
        "external_identities",
        &["id", "user_id", "issuer", "subject", "created_at"],
    ),
    (
        "sessions",
        &[
            "id_hash",
            "user_id",
            "created_at",
            "last_seen_at",
            "expires_at",
            "reauthenticated_at",
        ],
    ),
    (
        "auth_tokens",
        &[
            "token_hash",
            "user_id",
            "purpose",
            "created_at",
            "expires_at",
            "consumed_at",
        ],
    ),
    (
        "oauth_flows",
        &[
            "state_hash",
            "nonce_hash",
            "pkce_verifier_ciphertext",
            "intent",
            "session_id",
            "return_to",
            "created_at",
            "expires_at",
            "consumed_at",
        ],
    ),
    (
        "shows",
        &[
            "id",
            "tmdb_id",
            "title",
            "first_air_year",
            "genres",
            "synopsis",
            "poster_path",
            "status",
            "catalog_revision",
            "fetched_at",
            "complete_import",
            "created_at",
        ],
    ),
    (
        "seasons",
        &["id", "tmdb_id", "show_id", "number", "archived_at"],
    ),
    (
        "episodes",
        &[
            "id",
            "tmdb_id",
            "show_id",
            "season_id",
            "number",
            "title",
            "overview",
            "still_path",
            "air_date",
            "release_timezone",
            "archived_at",
        ],
    ),
    (
        "library_entries",
        &[
            "user_id",
            "show_id",
            "saved",
            "status",
            "saved_at",
            "revision",
            "updated_at",
        ],
    ),
    ("tracking_show_state", &["user_id", "show_id", "revision"]),
    (
        "mutation_actions",
        &[
            "id",
            "user_id",
            "show_id",
            "kind",
            "created_at",
            "undo_until",
            "undone_at",
            "undo_result",
        ],
    ),
    (
        "mutation_changes",
        &[
            "action_id",
            "entity",
            "entity_key",
            "field",
            "before_value",
            "after_revision",
        ],
    ),
    (
        "episode_progress",
        &[
            "user_id",
            "episode_id",
            "watched",
            "revision",
            "last_action_id",
            "updated_at",
        ],
    ),
    (
        "idempotency_records",
        &[
            "user_id",
            "key",
            "request_hash",
            "response",
            "created_at",
            "expires_at",
        ],
    ),
    (
        "catchup_previews",
        &[
            "id",
            "user_id",
            "show_id",
            "endpoint_episode_id",
            "episode_ids",
            "revisions",
            "catalog_revision",
            "created_at",
            "expires_at",
        ],
    ),
    (
        "follows",
        &["follower_id", "followee_id", "state", "created_at"],
    ),
    ("blocks", &["blocker_id", "blocked_id", "created_at"]),
    (
        "activity_events",
        &[
            "id",
            "actor_id",
            "kind",
            "show_id",
            "episode_id",
            "created_at",
            "action_id",
        ],
    ),
    (
        "discussions",
        &["id", "host_id", "episode_id", "created_at"],
    ),
    (
        "comments",
        &[
            "id",
            "discussion_id",
            "author_id",
            "body",
            "created_at",
            "removed_at",
            "removed_by",
        ],
    ),
    (
        "reports",
        &[
            "id",
            "reporter_id",
            "comment_id",
            "reason",
            "state",
            "created_at",
            "reviewed_by",
            "reviewed_at",
        ],
    ),
    ("hidden_comments", &["user_id", "comment_id"]),
    (
        "notifications",
        &[
            "id",
            "user_id",
            "kind",
            "actor_id",
            "follow_requester_id",
            "created_at",
            "read_at",
        ],
    ),
    (
        "outbox",
        &[
            "id",
            "kind",
            "payload",
            "dedupe_key",
            "created_at",
            "available_at",
            "attempts",
            "delivered_at",
        ],
    ),
    (
        "jobs",
        &[
            "id",
            "user_id",
            "kind",
            "state",
            "payload",
            "result_path",
            "created_at",
            "updated_at",
            "expires_at",
            "attempts",
            "lease_until",
        ],
    ),
    (
        "reveal_grants",
        &[
            "session_id",
            "user_id",
            "scope",
            "resource_id",
            "created_at",
            "expires_at",
        ],
    ),
    (
        "moderation_audit",
        &["id", "operator_id", "report_id", "action", "created_at"],
    ),
];

async fn public_tables(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Runs `sql` and asserts PostgreSQL rejects it with `code` (SQLSTATE).
async fn rejects(pool: &PgPool, code: &str, sql: &str) {
    let error = sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .expect_err(&format!("expected SQLSTATE {code} for: {sql}"));
    let actual = error.as_database_error().and_then(|e| e.code());
    assert_eq!(
        actual.as_deref(),
        Some(code),
        "expected SQLSTATE {code}, got {actual:?} for: {sql}"
    );
}

/// Runs a delete that an `ON DELETE RESTRICT` reference must block.
async fn rejects_restrict(pool: &PgPool, sql: &str) {
    let error = sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .expect_err(&format!("expected a restrict violation for: {sql}"));
    let actual = error.as_database_error().and_then(|e| e.code());
    // PostgreSQL 18 reports ON DELETE RESTRICT as restrict_violation (23001); 17 uses 23503.
    assert!(
        matches!(actual.as_deref(), Some("23001" | "23503")),
        "expected a restrict violation, got {actual:?} for: {sql}"
    );
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn migrations_apply_in_order_and_match_c02(pool: PgPool) {
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(versions, (1..=7).collect::<Vec<_>>());

    let mut expected: Vec<_> = SCHEMA.iter().map(|(table, _)| table.to_string()).collect();
    expected.sort();
    assert_eq!(public_tables(&pool).await, expected);

    for (table, columns) in SCHEMA {
        let actual: Vec<String> = sqlx::query_scalar(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(actual, *columns, "columns of {table}");
    }

    // Application IDs are UUIDs and timestamps are timestamptz everywhere.
    let wrong_types = count(
        &pool,
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'public'
           AND table_name <> '_sqlx_migrations'
           AND ((column_name = 'id' OR column_name LIKE '%\\_id' ESCAPE '\\')
                AND column_name NOT IN ('tmdb_id', 'session_id') AND data_type <> 'uuid'
             OR column_name LIKE '%\\_at' ESCAPE '\\' AND data_type <> 'timestamp with time zone')",
    )
    .await;
    assert_eq!(wrong_types, 0);
}

#[sqlx::test]
async fn development_rollback_reverts_every_migration(pool: PgPool) {
    MIGRATOR.undo(&pool, 0).await.unwrap();
    assert!(public_tables(&pool).await.is_empty());
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM pg_proc WHERE pronamespace = 'public'::regnamespace"
        )
        .await,
        0
    );
    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(public_tables(&pool).await.len(), SCHEMA.len());
}

#[sqlx::test(fixtures(path = "schema_fixtures", scripts("people", "catalog")))]
async fn constraints_reject_duplicates_self_edges_and_invalid_values(pool: PgPool) {
    // Duplicate provider and account identities.
    rejects(&pool, "23505", "INSERT INTO external_identities (user_id, issuer, subject)
        VALUES ('00000000-0000-4000-8000-00000000a0a1', 'https://accounts.google.com', 'fixture-subject-ben')").await;
    rejects(&pool, "23505", "INSERT INTO external_identities (user_id, issuer, subject)
        VALUES ('00000000-0000-4000-8000-00000000b0b1', 'https://accounts.google.com', 'another-subject')").await;
    rejects(
        &pool,
        "23505",
        "INSERT INTO users (normalized_email) VALUES ('ana@scenecask.test')",
    )
    .await;
    rejects(
        &pool,
        "23505",
        "INSERT INTO users (normalized_email, handle) VALUES ('x@scenecask.test', 'ana_r')",
    )
    .await;
    rejects(
        &pool,
        "23505",
        "INSERT INTO shows (tmdb_id, title, fetched_at) VALUES (900001, 'Copy', now())",
    )
    .await;
    rejects(
        &pool,
        "23505",
        "INSERT INTO episodes (tmdb_id, show_id, season_id, number)
        VALUES (910001, '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000005b1', 9)",
    )
    .await;
    rejects(
        &pool,
        "23505",
        "INSERT INTO seasons (tmdb_id, show_id, number)
        VALUES (905001, '00000000-0000-4000-8000-0000000005a1', 7)",
    )
    .await;
    rejects(
        &pool,
        "23P01",
        "INSERT INTO seasons (tmdb_id, show_id, number)
        VALUES (905009, '00000000-0000-4000-8000-0000000005a1', 1)",
    )
    .await;

    // Emails are stored in full Unicode casefold, whatever the database collation.
    exec(
        &pool,
        "INSERT INTO users (normalized_email) VALUES ('strasse@scenecask.test'), ('éva@scenecask.test')",
    )
    .await;

    // Follow and block graph.
    let follow = format!(
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ('{ANA}', '{BEN}', 'approved')"
    );
    exec(&pool, &follow).await;
    rejects(&pool, "23505", &follow).await;
    rejects(&pool, "23514", &format!("INSERT INTO follows (follower_id, followee_id, state) VALUES ('{ANA}', '{ANA}', 'pending')")).await;
    rejects(
        &pool,
        "23514",
        &format!("INSERT INTO blocks (blocker_id, blocked_id) VALUES ('{BEN}', '{BEN}')"),
    )
    .await;

    // Invalid enumerated values. Caught up and Completed are computed, never stored.
    for sql in [
        format!("UPDATE users SET visibility = 'friends' WHERE id = '{ANA}'"),
        format!("UPDATE users SET role = 'admin' WHERE id = '{ANA}'"),
        format!("UPDATE users SET handle = 'Ana!' WHERE id = '{ANA}'"),
        format!("UPDATE follows SET state = 'declined' WHERE follower_id = '{ANA}'"),
        format!(
            "INSERT INTO library_entries (user_id, show_id, status, saved_at, revision) VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 'completed', now(), 1)"
        ),
        format!(
            "INSERT INTO library_entries (user_id, show_id, status, saved_at, revision) VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 'caught_up', now(), 1)"
        ),
        format!(
            "INSERT INTO library_entries (user_id, show_id, saved, revision) VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', true, 1)"
        ),
        // A stored entry starts at revision 1; revision 0 means no row.
        format!(
            "INSERT INTO library_entries (user_id, show_id, saved_at, revision) VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', now(), 0)"
        ),
        "UPDATE shows SET status = 'airing'".to_owned(),
        format!(
            "UPDATE password_credentials SET argon2_hash = 'plaintext' WHERE user_id = '{ANA}'"
        ),
        format!(
            "INSERT INTO auth_tokens (token_hash, user_id, purpose, expires_at) VALUES ('\\x01', '{ANA}', 'login', now() + interval '1 hour')"
        ),
        format!("INSERT INTO jobs (user_id, kind, state) VALUES ('{ANA}', 'export', 'done')"),
        "INSERT INTO jobs (kind) VALUES ('export')".to_owned(),
        "INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent, expires_at) VALUES ('\\x01', '\\x02', '\\x03', 'link', now() + interval '10 minutes')".to_owned(),
        "INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent, return_to, expires_at) VALUES ('\\x01', '\\x02', '\\x03', 'signin', '//evil.test', now() + interval '10 minutes')".to_owned(),
        // Browsers strip tabs and newlines, which would turn these into '//evil.test'.
        "INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent, return_to, expires_at) VALUES ('\\x01', '\\x02', '\\x03', 'signin', '/\t/evil.test', now() + interval '10 minutes')".to_owned(),
        "INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent, return_to, expires_at) VALUES ('\\x01', '\\x02', '\\x03', 'signin', '/\n/evil.test', now() + interval '10 minutes')".to_owned(),
        "INSERT INTO users (normalized_email) VALUES ('Ana@scenecask.test')".to_owned(),
        "INSERT INTO users (normalized_email) VALUES ('ÉVA@scenecask.test')".to_owned(),
        "INSERT INTO users (normalized_email) VALUES ('straße@scenecask.test')".to_owned(),
        // Release state uses AT TIME ZONE, so an unresolvable zone would break every read.
        "UPDATE episodes SET release_timezone = 'America/Bogus' WHERE tmdb_id = 910001".to_owned(),
        format!("INSERT INTO jobs (user_id, kind, state) VALUES ('{ANA}', 'export', 'running')"),
        "INSERT INTO jobs (kind) VALUES ('delete')".to_owned(),
    ] {
        rejects(&pool, "23514", &sql).await;
    }
    exec(
        &pool,
        "UPDATE episodes SET release_timezone = 'America/Bogota' WHERE tmdb_id = 910001",
    )
    .await;
}

#[sqlx::test(fixtures(path = "schema_fixtures", scripts("people", "catalog")))]
async fn discussion_reports_jobs_and_grants_enforce_lifecycle_rules(pool: PgPool) {
    exec(&pool, &format!("
        INSERT INTO discussions (id, host_id, episode_id)
        VALUES ('00000000-0000-4000-8000-0000000000d1', '{ANA}', '00000000-0000-4000-8000-0000000005e1');
        INSERT INTO comments (id, discussion_id, author_id, body)
        VALUES ('00000000-0000-4000-8000-0000000000c1', '00000000-0000-4000-8000-0000000000d1', '{BEN}', 'That ending surprised me.');
        INSERT INTO reports (reporter_id, comment_id, reason)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000000c1', 'later_episode');
        INSERT INTO jobs (user_id, kind) VALUES ('{ANA}', 'export');
    ")).await;

    rejects(
        &pool,
        "23505",
        &format!(
            "INSERT INTO discussions (host_id, episode_id)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005e1')"
        ),
    )
    .await;
    // Live comments need a body; removed comments must not keep one.
    rejects(&pool, "23514", "INSERT INTO comments (discussion_id, body) VALUES ('00000000-0000-4000-8000-0000000000d1', NULL)").await;
    rejects(&pool, "23514", "UPDATE comments SET removed_at = now()").await;
    rejects(&pool, "23514", "INSERT INTO comments (discussion_id, body) VALUES ('00000000-0000-4000-8000-0000000000d1', '')").await;
    rejects(
        &pool,
        "23514",
        &format!("UPDATE reports SET reason = 'boring' WHERE reporter_id = '{ANA}'"),
    )
    .await;

    // One open report per reporter+comment; a new one is allowed after review.
    let report = format!(
        "INSERT INTO reports (reporter_id, comment_id, reason)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000000c1', 'spam')"
    );
    rejects(&pool, "23505", &report).await;
    rejects(&pool, "23514", "UPDATE reports SET state = 'resolved'").await;
    exec(
        &pool,
        "UPDATE reports SET state = 'dismissed', reviewed_at = now()",
    )
    .await;
    exec(&pool, &report).await;

    // One active export per user; a finished export frees the slot.
    let export = format!("INSERT INTO jobs (user_id, kind) VALUES ('{ANA}', 'export')");
    rejects(&pool, "23505", &export).await;
    exec(
        &pool,
        "UPDATE jobs SET state = 'running', lease_until = now() + interval '5 minutes'",
    )
    .await;
    rejects(&pool, "23514", "UPDATE jobs SET lease_until = NULL").await;
    exec(
        &pool,
        "UPDATE jobs SET state = 'ready', result_path = 'exports/fixture.json'",
    )
    .await;
    exec(&pool, &export).await;
    // A deletion job is created with its user and cannot drop it while the user exists.
    rejects(&pool, "23514", "INSERT INTO jobs (kind, state, lease_until) VALUES ('delete', 'running', now() + interval '5 minutes')").await;
    exec(&pool, &format!("INSERT INTO jobs (user_id, kind, state, lease_until) VALUES ('{ANA}', 'delete', 'running', now() + interval '5 minutes')")).await;
    rejects(
        &pool,
        "23514",
        "UPDATE jobs SET user_id = NULL WHERE kind = 'delete'",
    )
    .await;
    exec(&pool, "DELETE FROM jobs WHERE kind = 'delete'").await;
    // Only a claimed deletion job may lose its user: erasing a user whose job is still queued fails.
    exec(&pool, "
        INSERT INTO users (id, normalized_email) VALUES ('00000000-0000-4000-8000-00000000d0d1', 'dee@scenecask.test');
        INSERT INTO jobs (user_id, kind) VALUES ('00000000-0000-4000-8000-00000000d0d1', 'delete');
    ").await;
    rejects(
        &pool,
        "23514",
        "DELETE FROM users WHERE id = '00000000-0000-4000-8000-00000000d0d1'",
    )
    .await;
    exec(
        &pool,
        "
        DELETE FROM jobs WHERE user_id = '00000000-0000-4000-8000-00000000d0d1';
        DELETE FROM users WHERE id = '00000000-0000-4000-8000-00000000d0d1';
    ",
    )
    .await;

    // Reveal grants are bound to a session, capped at 12 hours and revoked with it.
    exec(&pool, &format!("
        INSERT INTO sessions (id_hash, user_id, expires_at) VALUES ('\\xaa', '{ANA}', now() + interval '7 days');
        INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{ANA}', 'discussion', '00000000-0000-4000-8000-0000000000d1', now() + interval '12 hours');
        INSERT INTO oauth_flows (state_hash, nonce_hash, pkce_verifier_ciphertext, intent, session_id, expires_at)
        VALUES ('\\x01', '\\x02', '\\x03', 'reauth', '\\xaa', now() + interval '10 minutes');
    ")).await;
    rejects(&pool, "23505", &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{ANA}', 'discussion', '00000000-0000-4000-8000-0000000000d1', now() + interval '1 hour')")).await;
    rejects(&pool, "23514", &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{ANA}', 'episode_details', '00000000-0000-4000-8000-0000000005e2', now() + interval '13 hours')")).await;
    rejects(&pool, "23514", &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{ANA}', 'everything', '00000000-0000-4000-8000-0000000005e2', now() + interval '1 hour')")).await;
    // The cap counts from insertion: the database stamps created_at, so a future value cannot
    // stretch the grant.
    rejects(&pool, "23514", &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, created_at, expires_at)
        VALUES ('\\xaa', '{ANA}', 'episode_details', '00000000-0000-4000-8000-0000000005e2', now() + interval '30 days', now() + interval '30 days 12 hours')")).await;
    rejects(
        &pool,
        "23514",
        "UPDATE reveal_grants SET created_at = now() + interval '30 days', expires_at = now() + interval '30 days 1 hour'",
    )
    .await;
    // created_at is stamped from the wall clock, so a grant written later in a transaction with
    // expires_at derived from an earlier clock reading is accepted.
    exec(&pool, &format!("
        BEGIN;
        SELECT pg_sleep(0.05);
        INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, created_at, expires_at)
        SELECT '\\xaa', '{ANA}', 'episode_details', '00000000-0000-4000-8000-0000000005e2', t, t + interval '12 hours'
        FROM (SELECT clock_timestamp() AS t) AS clock;
        COMMIT;
    ")).await;
    // A writer's created_at from a host clock running ahead is replaced, not rejected.
    exec(&pool, &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, created_at, expires_at)
        VALUES ('\\xaa', '{ANA}', 'discussion', '00000000-0000-4000-8000-0000000000d2', now() + interval '200 milliseconds', now() + interval '12 hours')")).await;
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM reveal_grants
             WHERE resource_id = '00000000-0000-4000-8000-0000000000d2' AND created_at <= clock_timestamp()",
        )
        .await,
        1
    );
    // A grant must belong to the session's owner.
    rejects(&pool, "23503", &format!("INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{BEN}', 'discussion', '00000000-0000-4000-8000-0000000000d9', now() + interval '1 hour')")).await;
    exec(&pool, "DELETE FROM sessions WHERE id_hash = '\\xaa'").await;
    assert_eq!(count(&pool, "SELECT count(*) FROM reveal_grants").await, 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM oauth_flows").await, 0);

    // A follow-request notification needs its follow edge; retries cannot duplicate it.
    let notification = format!(
        "INSERT INTO notifications (user_id, kind, actor_id) VALUES ('{ANA}', 'follow_request', '{BEN}')"
    );
    rejects(&pool, "23503", &notification).await;
    let request = format!(
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ('{BEN}', '{ANA}', 'pending')"
    );
    exec(&pool, &request).await;
    exec(&pool, &notification).await;
    rejects(&pool, "23505", &notification).await;
    // Approval keeps it; unfollowing removes it, so a later request notifies again.
    exec(
        &pool,
        "UPDATE follows SET state = 'approved'; UPDATE notifications SET read_at = now()",
    )
    .await;
    assert_eq!(count(&pool, "SELECT count(*) FROM notifications").await, 1);
    exec(&pool, "DELETE FROM follows").await;
    assert_eq!(count(&pool, "SELECT count(*) FROM notifications").await, 0);
    exec(&pool, &request).await;
    exec(&pool, &notification).await;
    // A follow request always names its actor, so none can exist without a follow edge.
    rejects(
        &pool,
        "23514",
        &format!("INSERT INTO notifications (user_id, kind) VALUES ('{ANA}', 'follow_request')"),
    )
    .await;
    // Erasing the requester removes the pending request notification with the edge.
    exec(&pool, &format!("
        INSERT INTO follows (follower_id, followee_id, state) VALUES ('{CLEO}', '{ANA}', 'pending');
        INSERT INTO notifications (user_id, kind, actor_id) VALUES ('{ANA}', 'follow_request', '{CLEO}');
    ")).await;
    exec(&pool, &format!("DELETE FROM users WHERE id = '{CLEO}'")).await;
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM notifications WHERE actor_id = '{CLEO}'")
        )
        .await,
        0
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM notifications").await, 1);
}

#[sqlx::test(fixtures(path = "schema_fixtures", scripts("people", "catalog")))]
async fn catalog_corrections_preserve_progress_and_revisions_only_increase(pool: PgPool) {
    const SHOW: &str = "00000000-0000-4000-8000-0000000005a1";
    const S1: &str = "00000000-0000-4000-8000-0000000005b1";
    exec(&pool, &format!("
        INSERT INTO mutation_actions (id, user_id, show_id, kind, undo_until)
        VALUES ('00000000-0000-4000-8000-0000000000a1', '{ANA}', '00000000-0000-4000-8000-0000000005a1', 'episode_watched', now() + interval '10 minutes');
        INSERT INTO episode_progress (user_id, episode_id, watched, revision, last_action_id)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005e2', true, 1, '00000000-0000-4000-8000-0000000000a1');
        INSERT INTO tracking_show_state (user_id, show_id, revision)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 1);
    ")).await;

    // Duplicate active numbering is rejected; a deferred swap inside one import succeeds.
    rejects(
        &pool,
        "23P01",
        &format!("INSERT INTO episodes (tmdb_id, show_id, season_id, number) VALUES (919999, '{SHOW}', '{S1}', 2)"),
    )
    .await;
    exec(
        &pool,
        "
        BEGIN;
        SET CONSTRAINTS episodes_active_season_number_key DEFERRED;
        UPDATE episodes SET number = 3 WHERE tmdb_id = 910002;
        UPDATE episodes SET number = 2 WHERE tmdb_id = 910003;
        COMMIT;
    ",
    )
    .await;
    // Season numbers can be swapped the same way, then swapped back in one statement.
    exec(
        &pool,
        &format!(
            "
        BEGIN;
        SET CONSTRAINTS seasons_active_show_number_key DEFERRED;
        UPDATE seasons SET number = 0 WHERE id = '{S1}';
        UPDATE seasons SET number = 1 WHERE id = '00000000-0000-4000-8000-0000000005b0';
        COMMIT;
    "
        ),
    )
    .await;
    assert_eq!(
        count(
            &pool,
            &format!("SELECT number::bigint FROM seasons WHERE id = '{S1}'")
        )
        .await,
        0
    );
    exec(&pool, "UPDATE seasons SET number = 1 - number").await;
    // An archived episode keeps its number and history; a new active episode may reuse it.
    exec(
        &pool,
        &format!(
            "
        UPDATE episodes SET archived_at = now() WHERE tmdb_id = 910002;
        INSERT INTO episodes (tmdb_id, show_id, season_id, number) VALUES (919999, '{SHOW}', '{S1}', 3);
    "
        ),
    )
    .await;
    // A season the provider removes is archived the same way, so a recreated season under a new
    // tmdb_id can take its number while the archived row keeps its episodes and history.
    exec(
        &pool,
        &format!(
            "
        UPDATE seasons SET archived_at = now() WHERE tmdb_id = 905000;
        INSERT INTO seasons (tmdb_id, show_id, number) VALUES (905100, '{SHOW}', 0);
    "
        ),
    )
    .await;
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM episode_progress WHERE user_id = '{ANA}' AND watched")
        )
        .await,
        1
    );

    // Catalog rows with history cannot be deleted.
    rejects_restrict(&pool, "DELETE FROM episodes WHERE tmdb_id = 910002").await;
    rejects_restrict(&pool, "DELETE FROM shows").await;

    // Revisions move forward, including true→false transitions, never back.
    exec(
        &pool,
        "UPDATE episode_progress SET watched = false, revision = 2",
    )
    .await;
    rejects(
        &pool,
        "23514",
        "UPDATE episode_progress SET watched = true, revision = 1",
    )
    .await;
    // A watched change must advance the revision; clearing the action link need not.
    rejects(&pool, "23514", "UPDATE episode_progress SET watched = true").await;
    exec(&pool, "UPDATE episode_progress SET last_action_id = NULL").await;
    exec(
        &pool,
        &format!(
            "
        INSERT INTO library_entries (user_id, show_id, status, saved_at, revision)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 'watching', now(), 1);
    "
        ),
    )
    .await;
    rejects(
        &pool,
        "23514",
        "UPDATE library_entries SET status = 'on_hold'",
    )
    .await;
    rejects(&pool, "23514", "UPDATE library_entries SET saved = false").await;
    exec(
        &pool,
        "UPDATE library_entries SET status = 'on_hold', revision = 2",
    )
    .await;
    rejects(
        &pool,
        "23514",
        "UPDATE tracking_show_state SET revision = 0",
    )
    .await;
    rejects(&pool, "23514", "UPDATE shows SET catalog_revision = 0").await;
    exec(
        &pool,
        "UPDATE shows SET catalog_revision = catalog_revision",
    )
    .await;

    // A misspelled revision column fails when the trigger is created.
    exec(
        &pool,
        "CREATE TABLE misconfigured (revision bigint NOT NULL)",
    )
    .await;
    rejects(
        &pool,
        "42703",
        "CREATE TRIGGER misconfigured_revision_forward BEFORE UPDATE ON misconfigured
            FOR EACH ROW WHEN (NEW.revison < OLD.revison)
            EXECUTE FUNCTION reject_update('revison', 'revision must not decrease')",
    )
    .await;

    // Previews and activity cannot pair a show with another show's episode.
    exec(
        &pool,
        "
        INSERT INTO shows (id, tmdb_id, title, fetched_at)
        VALUES ('00000000-0000-4000-8000-0000000006a1', 900002, 'Other Show', now());
        INSERT INTO seasons (id, tmdb_id, show_id, number)
        VALUES ('00000000-0000-4000-8000-0000000006b1', 925001, '00000000-0000-4000-8000-0000000006a1', 1);
        INSERT INTO episodes (id, tmdb_id, show_id, season_id, number)
        VALUES ('00000000-0000-4000-8000-0000000006e1', 920001, '00000000-0000-4000-8000-0000000006a1', '00000000-0000-4000-8000-0000000006b1', 1);
    ",
    )
    .await;
    rejects(&pool, "23503", &format!("INSERT INTO catchup_previews (user_id, show_id, endpoint_episode_id, episode_ids, revisions, catalog_revision, expires_at)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000006e1', '[]', '{{}}', 1, now() + interval '5 minutes')")).await;
    rejects(&pool, "23503", &format!("INSERT INTO activity_events (actor_id, kind, show_id, episode_id)
        VALUES ('{ANA}', 'episode_watched', '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000006e1')")).await;
    exec(&pool, &format!("INSERT INTO activity_events (actor_id, kind, show_id, episode_id)
        VALUES ('{ANA}', 'episode_watched', '00000000-0000-4000-8000-0000000006a1', '00000000-0000-4000-8000-0000000006e1')")).await;
    rejects(
        &pool,
        "23503",
        "UPDATE activity_events SET show_id = '00000000-0000-4000-8000-0000000005a1'",
    )
    .await;
    // Catalog corrections cannot move a season or an episode to another show; an episode may
    // move between seasons of the same show.
    rejects(
        &pool,
        "23514",
        "UPDATE seasons SET show_id = '00000000-0000-4000-8000-0000000006a1'
         WHERE id = '00000000-0000-4000-8000-0000000005b1'",
    )
    .await;
    rejects(
        &pool,
        "23503",
        "UPDATE episodes SET season_id = '00000000-0000-4000-8000-0000000005b1'
         WHERE id = '00000000-0000-4000-8000-0000000006e1'",
    )
    .await;
    rejects(
        &pool,
        "23514",
        "UPDATE episodes SET show_id = '00000000-0000-4000-8000-0000000005a1',
             season_id = '00000000-0000-4000-8000-0000000005b1'
         WHERE id = '00000000-0000-4000-8000-0000000006e1'",
    )
    .await;
    exec(
        &pool,
        "UPDATE episodes SET season_id = '00000000-0000-4000-8000-0000000005b1', number = 9
         WHERE id = '00000000-0000-4000-8000-0000000005e0'",
    )
    .await;

    // Catalog changes that can affect a catch-up preview advance catalog_revision; display-only
    // edits do not.
    let catalog_revision = format!("SELECT catalog_revision FROM shows WHERE id = '{SHOW}'");
    let before = count(&pool, &catalog_revision).await;
    exec(
        &pool,
        "UPDATE episodes SET title = 'Retitled' WHERE tmdb_id = 910001",
    )
    .await;
    assert_eq!(count(&pool, &catalog_revision).await, before);
    for sql in [
        "UPDATE episodes SET air_date = '2024-02-02' WHERE tmdb_id = 910001".to_owned(),
        "UPDATE episodes SET archived_at = now() WHERE tmdb_id = 910001".to_owned(),
        "UPDATE seasons SET number = 5 WHERE tmdb_id = 905000".to_owned(),
        format!(
            "INSERT INTO episodes (tmdb_id, show_id, season_id, number) VALUES (919998, '{SHOW}', '{S1}', 10)"
        ),
        format!("INSERT INTO seasons (tmdb_id, show_id, number) VALUES (905002, '{SHOW}', 2)"),
        "UPDATE seasons SET archived_at = now() WHERE tmdb_id = 905002".to_owned(),
    ] {
        let previous = count(&pool, &catalog_revision).await;
        exec(&pool, &sql).await;
        assert!(
            count(&pool, &catalog_revision).await > previous,
            "catalog_revision did not advance for: {sql}"
        );
    }
    // The triggers run per statement, so a bulk import advances the revision once, not per row.
    let previous = count(&pool, &catalog_revision).await;
    exec(
        &pool,
        &format!(
            "INSERT INTO episodes (tmdb_id, show_id, season_id, number)
             VALUES (919990, '{SHOW}', '{S1}', 20), (919991, '{SHOW}', '{S1}', 21), (919992, '{SHOW}', '{S1}', 22)"
        ),
    )
    .await;
    assert_eq!(count(&pool, &catalog_revision).await, previous + 1);
}

#[sqlx::test(fixtures(path = "schema_fixtures", scripts("people", "catalog")))]
async fn user_deletion_erases_private_state_without_cascading_shared_content(pool: PgPool) {
    exec(&pool, &format!("
        INSERT INTO library_entries (user_id, show_id, status, saved_at, revision)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 'watching', now(), 1),
               ('{BEN}', '00000000-0000-4000-8000-0000000005a1', 'watching', now(), 1);
        INSERT INTO tracking_show_state (user_id, show_id, revision)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', 1),
               ('{BEN}', '00000000-0000-4000-8000-0000000005a1', 1);
        INSERT INTO mutation_actions (id, user_id, show_id, kind, undo_until)
        VALUES ('00000000-0000-4000-8000-0000000000a1', '{ANA}', '00000000-0000-4000-8000-0000000005a1', 'episode_watched', now() + interval '10 minutes'),
               ('00000000-0000-4000-8000-0000000000b1', '{BEN}', '00000000-0000-4000-8000-0000000005a1', 'episode_watched', now() + interval '10 minutes');
        INSERT INTO mutation_changes (action_id, entity, entity_key, field, before_value, after_revision)
        VALUES ('00000000-0000-4000-8000-0000000000a1', 'episode_progress', '00000000-0000-4000-8000-0000000005e1', 'watched', 'false', 1),
               ('00000000-0000-4000-8000-0000000000a1', 'library_entry', '00000000-0000-4000-8000-0000000005a1', 'saved', 'false', 1);
        INSERT INTO episode_progress (user_id, episode_id, watched, revision, last_action_id)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005e1', true, 1, '00000000-0000-4000-8000-0000000000a1'),
               ('{BEN}', '00000000-0000-4000-8000-0000000005e1', true, 1, '00000000-0000-4000-8000-0000000000b1');
        INSERT INTO activity_events (actor_id, kind, show_id, episode_id, action_id)
        VALUES ('{ANA}', 'episode_watched', '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000005e1', '00000000-0000-4000-8000-0000000000a1'),
               ('{BEN}', 'episode_watched', '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000005e1', '00000000-0000-4000-8000-0000000000b1');
        INSERT INTO idempotency_records (user_id, key, request_hash, response, expires_at)
        VALUES ('{ANA}', gen_random_uuid(), '\\x01', '{{}}', now() + interval '24 hours');
        INSERT INTO catchup_previews (user_id, show_id, endpoint_episode_id, episode_ids, revisions, catalog_revision, expires_at)
        VALUES ('{ANA}', '00000000-0000-4000-8000-0000000005a1', '00000000-0000-4000-8000-0000000005e2', '[]', '{{}}', 1, now() + interval '5 minutes');
        INSERT INTO follows (follower_id, followee_id, state) VALUES ('{ANA}', '{BEN}', 'approved'), ('{BEN}', '{ANA}', 'approved');
        INSERT INTO blocks (blocker_id, blocked_id) VALUES ('{ANA}', '{CLEO}');
        INSERT INTO notifications (user_id, kind, actor_id) VALUES ('{ANA}', 'follow_request', '{BEN}'), ('{BEN}', 'follow_request', '{ANA}');
        INSERT INTO sessions (id_hash, user_id, expires_at) VALUES ('\\xaa', '{ANA}', now() + interval '7 days');
        INSERT INTO reveal_grants (session_id, user_id, scope, resource_id, expires_at)
        VALUES ('\\xaa', '{ANA}', 'episode_details', '00000000-0000-4000-8000-0000000005e2', now() + interval '1 hour');
        INSERT INTO auth_tokens (token_hash, user_id, purpose, expires_at) VALUES ('\\xbb', '{ANA}', 'reset', now() + interval '30 minutes');
        INSERT INTO jobs (user_id, kind, state, result_path) VALUES ('{ANA}', 'export', 'ready', 'exports/ana.json');
        INSERT INTO jobs (id, user_id, kind, state, lease_until)
        VALUES ('00000000-0000-4000-8000-0000000000f1', '{ANA}', 'delete', 'running', now() + interval '5 minutes');

        -- Ana hosts D1 (Ben comments there); Ben hosts D2 (Ana comments there).
        INSERT INTO discussions (id, host_id, episode_id)
        VALUES ('00000000-0000-4000-8000-0000000000d1', '{ANA}', '00000000-0000-4000-8000-0000000005e1'),
               ('00000000-0000-4000-8000-0000000000d2', '{BEN}', '00000000-0000-4000-8000-0000000005e1');
        INSERT INTO comments (id, discussion_id, author_id, body)
        VALUES ('00000000-0000-4000-8000-0000000000c1', '00000000-0000-4000-8000-0000000000d1', '{BEN}', 'Ben in Ana''s thread'),
               ('00000000-0000-4000-8000-0000000000c2', '00000000-0000-4000-8000-0000000000d2', '{ANA}', 'Ana in Ben''s thread'),
               ('00000000-0000-4000-8000-0000000000c3', '00000000-0000-4000-8000-0000000000d2', '{BEN}', 'Ben in his own thread');
        INSERT INTO hidden_comments (user_id, comment_id) VALUES ('{ANA}', '00000000-0000-4000-8000-0000000000c3');
        INSERT INTO reports (id, reporter_id, comment_id, reason, state, reviewed_by, reviewed_at)
        VALUES ('00000000-0000-4000-8000-0000000000e1', '{ANA}', '00000000-0000-4000-8000-0000000000c3', 'abuse', 'dismissed', '{CLEO}', now()),
               ('00000000-0000-4000-8000-0000000000e2', '{BEN}', '00000000-0000-4000-8000-0000000000c2', 'spam', 'open', NULL, NULL);
        INSERT INTO moderation_audit (operator_id, report_id, action)
        VALUES ('{CLEO}', '00000000-0000-4000-8000-0000000000e1', 'dismiss');
    ")).await;

    // Export rows must be removed (with their files) before the account row.
    rejects(
        &pool,
        "23514",
        &format!("DELETE FROM users WHERE id = '{ANA}'"),
    )
    .await;

    // Live comments must be tombstoned before the author is erased. Both statements run in
    // one implicit transaction, so the export delete rolls back too.
    rejects(
        &pool,
        "23514",
        &format!(
            "DELETE FROM jobs WHERE user_id = '{ANA}' AND kind = 'export';
             DELETE FROM users WHERE id = '{ANA}'"
        ),
    )
    .await;

    // Deletion job: tombstone the user's shared comments, delete exports, then the account row.
    exec(&pool, &format!("
        BEGIN;
        UPDATE comments SET body = NULL, removed_at = now() WHERE author_id = '{ANA}' AND removed_at IS NULL;
        DELETE FROM jobs WHERE user_id = '{ANA}' AND kind = 'export';
        DELETE FROM users WHERE id = '{ANA}';
        COMMIT;
    ")).await;

    // The claimed deletion job survives, detached from the user. Lease recovery or a retry can
    // requeue it, and a worker can claim and finish it.
    for sql in [
        "UPDATE jobs SET state = 'queued', lease_until = NULL, attempts = attempts + 1",
        "UPDATE jobs SET state = 'running', lease_until = now() + interval '5 minutes'",
        "UPDATE jobs SET state = 'failed', lease_until = NULL",
        "UPDATE jobs SET state = 'queued'",
        "UPDATE jobs SET state = 'ready'",
    ] {
        exec(
            &pool,
            &format!("{sql} WHERE id = '00000000-0000-4000-8000-0000000000f1' AND user_id IS NULL"),
        )
        .await;
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM jobs WHERE kind = 'delete' AND state = 'ready'"
        )
        .await,
        1
    );

    // Every user-keyed private table is empty for Ana.
    for (table, column) in [
        ("users", "id"),
        ("password_credentials", "user_id"),
        ("external_identities", "user_id"),
        ("sessions", "user_id"),
        ("auth_tokens", "user_id"),
        ("library_entries", "user_id"),
        ("tracking_show_state", "user_id"),
        ("mutation_actions", "user_id"),
        ("episode_progress", "user_id"),
        ("idempotency_records", "user_id"),
        ("catchup_previews", "user_id"),
        ("follows", "follower_id"),
        ("follows", "followee_id"),
        ("blocks", "blocker_id"),
        ("activity_events", "actor_id"),
        ("hidden_comments", "user_id"),
        ("notifications", "user_id"),
        ("notifications", "actor_id"),
        ("jobs", "user_id"),
        ("reveal_grants", "user_id"),
        ("comments", "author_id"),
        ("reports", "reporter_id"),
        ("discussions", "host_id"),
    ] {
        let remaining = count(
            &pool,
            &format!("SELECT count(*) FROM {table} WHERE {column} = '{ANA}'"),
        )
        .await;
        assert_eq!(
            remaining, 0,
            "{table}.{column} still references the deleted user"
        );
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM mutation_changes").await,
        0
    );

    // Ben's private state and comments survive, including in the thread Ana hosted.
    for table in [
        "library_entries",
        "tracking_show_state",
        "episode_progress",
        "mutation_actions",
        "activity_events",
    ] {
        assert_eq!(
            count(
                &pool,
                &format!(
                    "SELECT count(*) FROM {table} WHERE {} = '{BEN}'",
                    if table == "activity_events" {
                        "actor_id"
                    } else {
                        "user_id"
                    }
                )
            )
            .await,
            1,
            "{table}"
        );
    }
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*) FROM comments WHERE author_id = '{BEN}' AND body IS NOT NULL"
            )
        )
        .await,
        2
    );
    // Ana's thread survives hostless (inaccessible); her comment remains as an anonymous tombstone.
    assert_eq!(count(&pool, "SELECT count(*) FROM discussions WHERE id = '00000000-0000-4000-8000-0000000000d1' AND host_id IS NULL").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM comments WHERE id = '00000000-0000-4000-8000-0000000000c2'
        AND author_id IS NULL AND body IS NULL AND removed_at IS NOT NULL"
        )
        .await,
        1
    );
    // Moderation history keeps anonymous report identifiers; Ben's open report stays.
    assert_eq!(count(&pool, "SELECT count(*) FROM reports").await, 2);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM moderation_audit").await,
        1
    );
    // Catalog is untouched.
    assert_eq!(count(&pool, "SELECT count(*) FROM episodes").await, 5);

    // Pruning expired undo actions keeps activity and progress, only clearing the link.
    exec(
        &pool,
        &format!("DELETE FROM mutation_actions WHERE user_id = '{BEN}'"),
    )
    .await;
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*) FROM activity_events WHERE actor_id = '{BEN}' AND action_id IS NULL"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*) FROM episode_progress WHERE user_id = '{BEN}' AND last_action_id IS NULL"
            )
        )
        .await,
        1
    );
}
