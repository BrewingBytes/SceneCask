# Migration registry

Owner: R02. Source of truth for SQLx migration numbers in `services/api/migrations/`. Feature issues must not invent migrations; request a number from the migration owner first.

## Allocation

| Version | Files | Module owner | Contents (C02) |
|---|---|---|---|
| 0001 | `0001_auth.{up,down}.sql` | auth | users, password_credentials, external_identities, sessions, auth_tokens, oauth_flows; shared `reject_revision_decrease()` trigger function |
| 0002 | `0002_catalog.{up,down}.sql` | catalog | shows, seasons, episodes |
| 0003 | `0003_library_tracking.{up,down}.sql` | library, tracking | library_entries, tracking_show_state, mutation_actions, mutation_changes, episode_progress, idempotency_records, catchup_previews |
| 0004 | `0004_social.{up,down}.sql` | social | follows, blocks, activity_events |
| 0005 | `0005_discussions.{up,down}.sql` | discussions, moderation | discussions, comments, reports, hidden_comments |
| 0006 | `0006_notifications_jobs.{up,down}.sql` | notifications, account, infrastructure | notifications, outbox, jobs |
| 0007 | `0007_policy.{up,down}.sql` | discussions, moderation | reveal_grants, moderation_audit |
| 0008+ | unallocated | — | Request from the migration owner; record here before merging |

## Policy

- **Forward-only in production.** Never edit or delete a migration once merged; SQLx checksums applied migrations and refuses changed files. Fix forward with a new number. Production rollback uses a backup restore or a forward fix, never a destructive down migration.
- **Development rollback.** Every migration has a `.down.sql` that reverts it exactly. It exists for local iteration and is exercised by `development_rollback_reverts_every_migration`.
- **Applying.** Migrations are embedded with `sqlx::migrate!()` (default path `services/api/migrations`). Wiring startup or a deployment step to run them belongs to the integration owners (R24/R38); until then use `sqlx migrate run --source services/api/migrations` with sqlx-cli.
- **Supported PostgreSQL.** Verified on 17.6 (CI) and 18.6 (Compose image).

## Conventions

- Application IDs are `uuid` with `gen_random_uuid()` defaults; provider IDs (`tmdb_id`) are unique `bigint` columns, never primary keys.
- Timestamps are `timestamptz`; release dates are `date`.
- Enumerations are `text` with `CHECK` constraints so invalid values fail in the database. Caught up and Completed are computed and have no stored status.
- Revision columns are `bigint`. A trigger rejects any update that lowers `library_entries.revision`, `tracking_show_state.revision`, `episode_progress.revision` or `shows.catalog_revision`.
- Secrets are stored only as hashes (`bytea`) or ciphertext. Never log credential, token, session or OAuth columns.

## Deletion and cascade boundaries

| Relationship | Rule | Why |
|---|---|---|
| User-private rows (credentials, identities, sessions, tokens, library, progress, actions, idempotency, previews, follows, blocks, activity, hidden comments, notifications, reveal grants) → users | `CASCADE` | Account erasure removes private state |
| jobs.user_id → users | `SET NULL`, with a `CHECK` that only a claimed `delete` job may lose its user | The deletion job keeps its row to record its outcome; remaining export rows block erasure until their files and rows are deleted |
| activity_events.action_id, episode_progress.last_action_id → mutation_actions | `SET NULL` | Pruning expired undo actions keeps activity and progress |
| sessions → reveal_grants (via `session_id, user_id`), oauth_flows | `CASCADE` | Logout, reset and expiry revoke grants and flows; a grant's user always owns its session |
| discussions.host_id, comments.author_id / removed_by, reports.reporter_id / reviewed_by, notifications.actor_id, moderation_audit.operator_id → users | `SET NULL` | Shared content and moderation history survive as anonymous records |
| comments → discussions, reports → comments, moderation_audit → reports | `RESTRICT` | Other users' comments and moderation history are never cascaded away |
| catalog references (seasons, episodes, progress, library, activity, discussions, previews) → shows / episodes | `RESTRICT` | Removed provider episodes are archived (`archived_at`); history is never deleted |

The account deletion job tombstones the user's comments (`body = NULL`, `removed_at = now()`) and deletes the user's export artifact files and their `jobs` rows before deleting the `users` row. The database rejects the `users` delete while export rows remain. A hostless discussion is inaccessible to everyone because access requires the host.

## Additions beyond the C02 column list

These columns and constraints are required by C03–C08 behavior and were added so feature owners do not need early schema changes:

| Addition | Reason |
|---|---|
| `external_identities` unique `(user_id, issuer)` | One Google identity per account (`GET /me/identities`) |
| `password_credentials.updated_at`, `auth_tokens.created_at` | Credential rotation audit; 60-second resend cooldown |
| `oauth_flows.return_to`, `created_at`; `pkce_verifier` stored as `pkce_verifier_ciphertext` | Allowlisted same-origin returnTo; encrypted verifier |
| `mutation_actions.show_id`, `undo_result` | Undo returns the show's LibraryItem; repeated undo returns the stored outcome |
| `library_entries.updated_at`, `jobs.created_at` / `updated_at`, `outbox.created_at`, `idempotency_records.created_at`, `catchup_previews.created_at`, `reveal_grants.created_at` | Operational timestamps and expiry checks |
| `episodes` active-number uniqueness is a deferrable exclusion constraint over non-archived rows | Numbering corrections can swap numbers in one import; archived episodes keep their number |
| `jobs` unique active `(user_id, kind)` for export/delete | One active export per user (C08) |
| `jobs` running state requires `lease_until` | A running job cannot be reclaimed immediately by another worker |
| `notifications` unique `(user_id, kind, actor_id)` | Follow retries cannot duplicate a notification; block cancels it by pair (C06) |
| `users.normalized_email` must be lowercase | Case variants cannot create duplicate accounts (C03 casefold) |
| `oauth_flows.return_to` rejects backslash, whitespace and control characters | Browsers strip tabs/newlines, which could rebuild a `//` open redirect |
| `sessions` unique `(id_hash, user_id)` | Target for the `reveal_grants` owner foreign key |
| `activity_events.action_id` nullable | Pruning expired undo actions keeps activity |
| `reveal_grants` expiry capped at 12 hours | C04 grant lifetime |

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
| Duplicate provider identity, email, handle, show/episode TMDB ID | Unique constraints | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Duplicate follow pair, self-follow, self-block | PK and `CHECK` | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Invalid visibility, role, handle, email case, follow state, library status (incl. computed states), show status, token purpose, job state/lease, OAuth intent/session/returnTo (incl. tab/newline) | `CHECK` | `constraints_reject_duplicates_self_edges_and_invalid_values` |
| Unique host+episode discussion; comment body/tombstone rules; report reason; one open report per reporter+comment | Constraints and partial unique index | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| One active export per user; running jobs hold a lease | Partial unique index, `CHECK` | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| One follow-request notification per requester | Partial unique index | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| Reveal grants unique per session+scope+resource, owned by the session's user, max 12h, revoked with session | PK, `CHECK`, `CASCADE` | `discussion_reports_jobs_and_grants_enforce_lifecycle_rules` |
| Numbering corrections preserve progress; archived numbers reusable | Deferrable exclusion constraint | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| Catalog rows with history cannot be deleted | `RESTRICT` | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| Revisions never decrease, including true→false→true; a misconfigured trigger fails loudly | Trigger | `catalog_corrections_preserve_progress_and_revisions_only_increase` |
| User deletion erases private state without cascading other users' comments/history; exports must go first; the deletion job survives; pruning undo actions keeps activity | Cascade boundaries above | `user_deletion_erases_private_state_without_cascading_shared_content` |

Cursor and filter indexes: library (`user_id, saved_at, show_id` where saved), episodes (`season_id, number, id`), follows (`followee_id, state, created_at, follower_id`), activity (`actor_id, created_at, id`), discussions (`episode_id, created_at, id`), comments (`discussion_id, created_at, id`), reports (`state, created_at, id`), notifications (`user_id, created_at, id`), plus expiry/claim indexes for sessions, tokens, OAuth flows, idempotency records, previews, outbox and jobs.
