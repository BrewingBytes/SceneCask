# Migration registry

Owner: R02. Source of truth for SQLx migration numbers in `services/api/migrations/`. Feature issues must not invent migrations; request a number from the migration owner first.

## Allocation

| Version | Files | Module owner | Contents (C02) |
|---|---|---|---|
| 0001 | `0001_auth.{up,down}.sql` | auth | users, password_credentials, external_identities, sessions, auth_tokens, oauth_flows; shared `reject_update(column, message)` guard trigger function |
| 0002 | `0002_catalog.{up,down}.sql` | catalog | shows, seasons, episodes; `advance_catalog_revision()` and `episodes_release_timezone_valid()` trigger functions |
| 0003 | `0003_library_tracking.{up,down}.sql` | library, tracking | library_entries, tracking_show_state, mutation_actions, mutation_changes, episode_progress, idempotency_records, catchup_previews |
| 0004 | `0004_social.{up,down}.sql` | social | follows, blocks, activity_events |
| 0005 | `0005_discussions.{up,down}.sql` | discussions, moderation | discussions, comments, reports, hidden_comments |
| 0006 | `0006_notifications_jobs.{up,down}.sql` | notifications, account, infrastructure | notifications, outbox, jobs; `jobs_delete_target_required()` trigger function |
| 0007 | `0007_policy.{up,down}.sql` | discussions, moderation | reveal_grants, moderation_audit; `reveal_grants_stamp_created_at()` trigger function |
| 0008+ | unallocated | — | Request from the migration owner; record here before merging |

## Policy

- **Forward-only in production.** Never edit or delete a migration once merged; SQLx checksums applied migrations and refuses changed files. Fix forward with a new number. Production rollback uses a backup restore or a forward fix, never a destructive down migration.
- **Development rollback.** Every migration has a `.down.sql` that reverts it exactly. It exists for local iteration and is exercised by `development_rollback_reverts_every_migration`.
- **Applying.** Migrations are embedded with `sqlx::migrate!()` (default path `services/api/migrations`). Wiring startup or a deployment step to run them belongs to the integration owners (R24/R38); until then use `sqlx migrate run --source services/api/migrations` with sqlx-cli.
- **Supported PostgreSQL.** Verified on 18.6, the image used by both CI and Compose. Other major versions are untested.

## Conventions

- Application IDs are `uuid` with `gen_random_uuid()` defaults; provider IDs (`tmdb_id`) are unique `bigint` columns, never primary keys.
- Timestamps are `timestamptz`; release dates are `date`.
- Enumerations are `text` with `CHECK` constraints so invalid values fail in the database. Caught up and Completed are computed and have no stored status.
- Revision columns are `bigint`. A `reject_update` trigger (its `WHEN` clause names the columns, so they are checked at `CREATE TRIGGER`) rejects any update that lowers `library_entries.revision`, `tracking_show_state.revision`, `episode_progress.revision` or `shows.catalog_revision`. On `library_entries` (`saved`, `status`, `saved_at`) and `episode_progress` (`watched`) it also rejects a state change that does not raise the revision. Stored `library_entries` and `episode_progress` rows start at revision 1 (an absent row reads as 0), so `mutation_changes.after_revision` can always name the written revision.
- `shows.catalog_revision` advances automatically: statement-level triggers on `seasons` (insert, number change or archival) and `episodes` (insert, or a change to season, number, air date, release timezone or archival) increment it once per affected show per statement, so a catch-up preview built on an older catalog fails as `PREVIEW_STALE` and a bulk import does not rewrite the show row once per episode. Title, overview and artwork edits do not advance it.
- Secrets are stored only as hashes (`bytea`) or ciphertext. Never log credential, token, session or OAuth columns.

## Deletion and cascade boundaries

| Relationship | Rule | Why |
|---|---|---|
| User-private rows (credentials, identities, sessions, tokens, library, progress, actions, idempotency, previews, follows, blocks, activity, hidden comments, notifications, reveal grants) → users | `CASCADE` | Account erasure removes private state |
| jobs.user_id → users | `SET NULL`, with a `CHECK` that only a `delete` job may lose its user and a trigger that lets it drop its user only while claimed (`running`) and once that user is erased | The deletion job keeps its row to record its outcome and can still be requeued by lease recovery or a retry; remaining export rows block erasure until their files and rows are deleted |
| activity_events.action_id, episode_progress.last_action_id → mutation_actions | `SET NULL` | Pruning expired undo actions keeps activity and progress |
| notifications (`follow_requester_id, user_id`) → follows (`follower_id, followee_id`) | `CASCADE` | Withdrawal, decline, follower removal, unfollow, block and erasure of either user delete the request notification with its edge, so a later request can notify again. `follow_requester_id` is generated from `actor_id` for `follow_request` only, so other kinds are not bound to a follow edge. `actor_id` is nullable per C02, but a `CHECK` requires it for `follow_request`; it has no separate users reference |
| sessions → reveal_grants (via `session_id, user_id`), oauth_flows | `CASCADE`; `oauth_flows.session_id` is indexed | Logout, reset and expiry revoke grants and flows; a grant's user always owns its session |
| discussions.host_id, comments.author_id / removed_by, reports.reporter_id / reviewed_by, moderation_audit.operator_id → users | `SET NULL`; each column is indexed so erasure does not scan | Shared content and moderation history survive as anonymous records; `comments` rejects a live body without an author |
| comments → discussions, reports → comments, moderation_audit → reports | `RESTRICT` | Other users' comments and moderation history are never cascaded away |
| catalog references (seasons, episodes, progress, library, activity, discussions, previews) → shows / episodes | `RESTRICT` | Removed provider seasons and episodes are archived (`archived_at`); history is never deleted |

The account deletion job tombstones the user's comments (`body = NULL`, `removed_at = now()`) and deletes the user's export artifact files and their `jobs` rows before deleting the `users` row. The database rejects the `users` delete while export rows remain or any of the user's comments still has a body. A hostless discussion is inaccessible to everyone because access requires the host.

## Additions beyond the C02 column list

These columns and constraints are required by C03–C08 behavior and were added so feature owners do not need early schema changes:

| Addition | Reason |
|---|---|
| `external_identities` unique `(user_id, issuer)` | One Google identity per account (`GET /me/identities`) |
| `password_credentials.updated_at`, `auth_tokens.created_at` | Credential rotation audit; 60-second resend cooldown |
| `oauth_flows.return_to`, `created_at`; `pkce_verifier` stored as `pkce_verifier_ciphertext` | Allowlisted same-origin returnTo; encrypted verifier |
| `mutation_actions.show_id`, `undo_result` | Undo returns the show's LibraryItem; repeated undo returns the stored outcome |
| `library_entries.revision` has no default and must be positive | The first write stores revision 1, matching `episode_progress` and `mutation_changes.after_revision > 0` |
| `library_entries.updated_at`, `jobs.created_at` / `updated_at`, `outbox.created_at`, `idempotency_records.created_at`, `catchup_previews.created_at`, `reveal_grants.created_at` | Operational timestamps and expiry checks |
| `episodes` active-number uniqueness is a deferrable exclusion constraint over non-archived rows | Numbering corrections can swap numbers in one import; archived episodes keep their number. It cannot be an `ON CONFLICT` arbiter: imports upsert episodes on `tmdb_id`, and collisions fail at commit |
| `seasons.tmdb_id` (unique) | C02 numbering corrections match by provider identity; imports upsert seasons on `tmdb_id`, never by number |
| `seasons.archived_at`; `(show_id, number)` uniqueness is a deferrable exclusion constraint over non-archived rows | A season the provider removes is archived (its rows are still referenced), and a season recreated under a new `tmdb_id` can reuse its number. Imports can swap season numbers in one transaction. It cannot be an `ON CONFLICT` arbiter, so imports lock the show row and upsert on `tmdb_id` |
| `episodes.show_id` with foreign key `(show_id, season_id)` → `seasons (show_id, id)` | Lets rows pairing a show with an episode reference `episodes (show_id, id)`, so PostgreSQL checks the pairing |
| Seasons and episodes never change `show_id`; episodes only move between seasons of the same show | Progress and history stay with their show through catalog corrections |
| `shows.catalog_revision` advanced by catalog triggers | Catch-up previews go stale whenever episode membership, order or release data change |
| `jobs` unique active `(user_id, kind)` for export/delete | One active export per user (C08) |
| `jobs` running state requires `lease_until` | A running job cannot be reclaimed immediately by another worker |
| `notifications.follow_requester_id` (generated), unique `(follow_requester_id, user_id)`, foreign key `(follow_requester_id, user_id)` → `follows`; `follow_request` requires `actor_id` | Follow retries cannot duplicate a notification; removing the edge (withdrawal, decline, block) cancels it (C06). The unique index also serves the cascade. Future kinds with an actor are not tied to a follow edge |
| `users.normalized_email` must equal its full Unicode casefold (`casefold(... COLLATE pg_unicode_fast)`) | Case variants cannot create duplicate accounts under any database collation (C03 casefold). The API must normalize with the same full casefold |
| `episodes.release_timezone` must resolve as a PostgreSQL time zone | An unrecognized name would make `AT TIME ZONE` fail every release-state read of the show |
| `oauth_flows.return_to` rejects backslash, whitespace and control characters | Browsers strip tabs/newlines, which could rebuild a `//` open redirect |
| `sessions` unique `(id_hash, user_id)` | Target for the `reveal_grants` owner foreign key |
| `activity_events.action_id` nullable | Pruning expired undo actions keeps activity |
| `reveal_grants` expiry capped at 12 hours; the database stamps `created_at` from its wall clock (`clock_timestamp()`) on insert or change | C04 grant lifetime, counted from insertion. A writer-supplied value is replaced, so neither a future value nor app-host clock skew affects the cap. Writers derive `expires_at` in SQL (for example `now() + interval '12 hours'`) |
| `comments` live body requires an author | Erasure must tombstone shared comments first (C08) |
| `catchup_previews (show_id, endpoint_episode_id)` and `activity_events (show_id, episode_id)` reference `episodes (show_id, id)` | Catch-up and activity cannot pair a show with another show's episode |

## Test database factory and fixtures

`#[sqlx::test]` creates a fresh database per test on the server named by `DATABASE_URL` (the role needs `CREATEDB`), applies every migration and drops the database afterwards. Load fixtures with `#[sqlx::test(fixtures(path = "schema_fixtures", scripts("people", "catalog")))]`.

| Fixture | Contents |
|---|---|
| `services/api/tests/schema_fixtures/people.sql` | Ana (private member, password placeholder), Ben (public member, Google identity), Cleo (operator) |
| `services/api/tests/schema_fixtures/catalog.sql` | Hollow Orchard: specials season with one special; season 1 with released, future and undated episodes |

Fixtures are development/test data only. Never load them into a deployment or real accounts.

## Schema coverage

Tests live in `services/api/tests/schema.rs`.

| Requirement | Enforced by | Test |
|---|---|---|
| All migrations apply in order on empty PostgreSQL; tables and columns match | 0001–0007 | `migrations_apply_in_order_and_match_c02` |
| UUID application IDs and timestamptz timestamps | Column types | `migrations_apply_in_order_and_match_c02` |
| Development rollback reverts cleanly and reapplies | `.down.sql` files | `development_rollback_reverts_every_migration` |
| Duplicate provider identity, email, handle, show/season/episode TMDB ID | Unique constraints | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Duplicate follow pair, self-follow, self-block | PK and `CHECK` | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Invalid visibility, role, handle, email casefold (incl. non-ASCII), release time zone, follow state, library status (incl. computed states) and revision 0, show status, token purpose, job state/lease, OAuth intent/session/returnTo (incl. tab/newline) | `CHECK` | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Unique host+episode discussion; comment body/tombstone rules; report reason; one open report per reporter+comment | Constraints and partial unique index | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| One active export per user; running jobs hold a lease; a deletion job keeps its user until erasure and only a claimed one may lose it | Partial unique index, `CHECK`, trigger | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| One follow-request notification per requester; it needs and leaves with its follow edge, including on the requester's erasure | Unique constraint, `CHECK` on actor, foreign key to `follows` | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| Reveal grants unique per session+scope+resource, owned by the session's user, max 12h from insertion (stamped from the wall clock, ignoring writer clock skew), revoked with session | PK, `CHECK`, trigger, `CASCADE` | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| Numbering corrections preserve progress; episode and season numbers can be swapped; archived episode and season numbers reusable | Deferrable exclusion and unique constraints | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| Catalog rows with history cannot be deleted | `RESTRICT` | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| Revisions never decrease and advance with every library or watched change, including true→false→true; catalog changes advance `catalog_revision` (once per statement) and display-only edits do not; a misspelled trigger column fails at creation | Triggers | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| Preview endpoint and activity episode belong to the row's show, including after catalog corrections; seasons and episodes cannot change show | Composite foreign keys, trigger | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| User deletion erases private state without cascading other users' comments/history; exports must go first; comments must be tombstoned first; the deletion job survives and can be requeued and finished; pruning undo actions keeps activity | Cascade boundaries above | `user_deletion_erases_private_state_without_cascading_shared_content` |

Cursor and filter indexes: library (`user_id, saved_at, show_id` where saved), active episodes (`season_id, number`, the exclusion constraint's index), follows (`followee_id, state, created_at, follower_id` and `follower_id, state, created_at, followee_id`), activity (`actor_id, created_at, id`), discussions (`episode_id, created_at, id`), comments (`discussion_id, created_at, id`), reports (`state, created_at, id`), notifications (`user_id, created_at, id`), plus expiry/claim indexes for sessions, tokens, OAuth flows (and `oauth_flows.session_id` for the session cascade), idempotency records, previews, outbox and jobs, and indexes on every `SET NULL` user reference for erasure.
