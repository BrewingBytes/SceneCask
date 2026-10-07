# R11 catalog boundary and integration handoff

Issue #7 implements C05 TV search and initial/repeated atomic import. Prerequisites
R02/PR #54 and R03/PR #56 are merged and present in the branch base. It also reuses
R06's merged session/error/security boundary (PR #61).

`TvProvider` separates provider HTTP from catalog persistence. `Tmdb::new(token)`
accepts a server-only TMDB API read-access token. The production API origin is fixed
at `https://api.themoviedb.org/3/`; credentials are sent in a sensitive bearer
header, never an image URL. Only fixture unit tests can select a localhost origin.
Do not log the token, internal `Catalog` fields or HTTP/database error objects.

## R24 registration

The feature exports guarded Axum handlers without changing the shared application
router. Register `provider::search_handler::<Tmdb>` at `GET /shows/search` with
`provider::SearchState { search: Arc::new(Search::new(provider.clone())), limiter:
security.limiter.clone(), rule: SEARCH_RULES[0] }`. Register
`import::import_handler::<Tmdb>` at `POST /shows/import` with
`Arc::new(Importer::new(pool, provider))` as state. Keep both states alive across
requests. Merge the routes, apply `middleware::apply` once, and nest under
`/api/v1`. The handlers use `VerifiedUser`; the security layers supply sessions,
request IDs, same-origin/CSRF/JSON validation, body limits, write limits and private
no-store headers. Search enforces its own configurable per-user search rule even
on cache hits. The HTTP tests mount these exact handlers with the real security
layers and PostgreSQL.

Keep token loading in the integration owner's configuration/startup code. No live
TMDB request is necessary for construction. Appropriate TMDB API access is still a
deployment input. No new schema, API contract, generated client or shared router
changes are needed. The user authorized the minimal supporting Cargo dependency,
lockfile and module-export edits in this session.

## Bounds and behavior

- Trimmed query: 2–100 Unicode characters, page 1–500 (default 1).
- Only `/search/tv`, with TV genres for title/year/genre/poster disambiguation.
- Successful search cache: 15 minutes, at most 512 query/page entries; errors are
  not cached. Configuration/TV genre cache: 24 hours. Caches are per API process.
- Connect timeout 2 seconds; request timeout 5 seconds; at most three attempts,
  jittered backoff for connection failures/5xx. Numeric and HTTP-date Retry-After
  are honored. A delay beyond 2 seconds fails immediately rather than retrying
  early. 429/timeout/provider schema failures produce fixed 502
  PROVIDER_UNAVAILABLE. The consumer can offer Retry. There is no raw provider
  body or submitted value in an error.
- Provider response body at most 4 MiB; search operation budget 20 seconds; complete
  catalog fetch budget 30 seconds, season list at most 1,000. A timeout or incomplete
  season/episode count aborts before any write transaction starts.
- Recent complete imports are reused for 24 hours. Stale imports fetch a validated
  full snapshot first, then take a namespaced transaction advisory lock on provider
  show ID and lock the show row. Lock wait is capped at 5 seconds, statements at
  10 seconds. Concurrent initial imports may fetch in parallel, but recheck freshness
  under the lock and converge on one application UUID.
- Missing provider rows are archived, never deleted. Matching seasons/episodes use
  stable provider IDs; upserts return the original app IDs and cannot move between
  shows. Deferrable ordering constraints allow numbering corrections. Both newly
  inserted and existing catalog changes roll back on identity/ordering conflicts.
- Complete-import flag and fetched timestamp become visible only after commit.
  Existing history/progress rows are never written or deleted by the importer.
- Missing/empty dates remain null; valid date-only releases have null reliable
  timezone. R13/R18 must apply the C05 UTC-midnight estimated fallback. Future dates
  are retained. Unknown status remains unknown. Specials are imported as season 0.
- Images use `/configuration`'s secure base and supported w500/original poster size.
  Missing or unsafe paths map to null; image links contain no credential. The
  importer response contains only `showId`, never episode metadata.

## Attribution and follow-ups

The adapter exposes `ATTRIBUTION`, `ATTRIBUTION_URL` and `ATTRIBUTION_GUIDE` for D04.
R24/settings UI must display the required approved TMDB logo alongside the text and
link. This backend issue does not add a logo or UI screen.

Official references checked 2026-10-07: [TV search](https://developer.themoviedb.org/reference/search-tv),
[TV details](https://developer.themoviedb.org/reference/tv-series-details),
[season details](https://developer.themoviedb.org/reference/tv-season-details),
[image configuration](https://developer.themoviedb.org/docs/image-basics), and
[attribution guidance](https://developer.themoviedb.org/docs/faq).

R12 (#10) can build the durable scheduled refresh worker on the provider/import
boundary after this PR merges. R18 (#17) and R19 (#18) have their R11 prerequisite
satisfied, but still require their other backlog dependencies. Scheduled refresh,
viewer-filtered catalog DTOs and live app registration remain with their owners.
No live-provider smoke check has been performed; CI uses local fixture HTTP only.

## Validation recorded 2026-10-07

Automated checks on this branch:

| Command | Actual result |
| --- | --- |
| `yarn lint` | Passed web lint, Rust formatting and all-target Clippy with warnings denied. |
| `yarn typecheck` | Passed. |
| `yarn test` with local PostgreSQL and SMTP capture configured | Passed all 52 API tests: 19 unit, 8 catalog integration/cache, 3 health, 2 mail, 6 schema, 14 session. Twelve tests are new catalog coverage (4 provider + 8 integration/cache); seven use freshly migrated PostgreSQL. |
| `yarn build` | Passed Next.js production build and locked Rust build. |
| `yarn api:check` | Passed OpenAPI validation, 20 contract/client tests, generated-client/mapping drift check. |
| `yarn test:e2e` with real API/PostgreSQL | Passed all 6 existing foundation/browser tests, including same-origin API health and 320/390/859/860/1440 layouts. These are regressions, not a new catalog browser journey. |
| `git diff --check` | Passed. |

Provider evidence is synthetic HTTP replay, not a live provider smoke check.
PostgreSQL evidence covers eight overlapping imports converging on one UUID,
initial and stale-import rollback after intermediate writes, stable IDs through
renumbering/archive/restore, retained watched history during provider failure,
auth/verification/CSRF/validation/no-store boundaries and per-user search throttling.
Transport tests cover timeout, 429 cooldown, 5xx, redirect, malformed and oversized
responses without payload/credential echo. No manual UI testing was required and
no UI files changed. Main application wiring is deliberately left to R24.
