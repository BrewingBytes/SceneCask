# R11 catalog boundary and integration handoff

Issue #7 implements C05 TV search and initial/repeated atomic import on top of the
R06 session/error/security boundary.

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
deployment input.

## Bounds and behavior

- Trimmed query: 2–100 Unicode characters, page 1–500 (default 1).
- Only `/search/tv`, with TV genres for title/year/genre/poster disambiguation.
- Successful search cache: 15 minutes, at most 512 query/page entries; errors are
  not cached. Configuration/TV genre cache: 24 hours. Caches are per API process.
- Connect timeout 2 seconds; request timeout 5 seconds; at most three attempts,
  jittered backoff for connection failures/5xx. Numeric and HTTP-date Retry-After
  are honored. A delay beyond 2 seconds ends the request rather than retrying
  early; a 429 then reports `ProviderError::RateLimited(delay)` (capped at one day)
  so the refresh worker can pause. 429/timeout/provider schema failures all map to
  fixed 502 PROVIDER_UNAVAILABLE for HTTP consumers, which can offer Retry. There is
  no raw provider body or submitted value in an error.
- Provider response body at most 4 MiB; search operation budget 20 seconds; complete
  catalog fetch budget 30 seconds, season list at most 1,000. A timeout or incomplete
  season/episode count aborts before any write transaction starts.
- Recent complete imports are reused for 24 hours. If refreshing a stale complete
  import fails (provider error, invalid or conflicting snapshot), the transaction
  rolls back and the stored show ID is returned; only a first import returns 502.
  Stale imports fetch a validated
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
  timezone. R13/R18 must apply the C05 UTC-midnight estimated fallback. Future
  dates are retained. Unknown status remains unknown. Specials are imported as
  season 0. Search skips unusable results and maps a malformed year to null.
- `catalog_revision` advances only through the 0002 triggers, so an unchanged
  refresh keeps catch-up previews valid.
- Images use `/configuration`'s secure base and supported w500/original poster size.
  Missing or unsafe paths map to null; image links contain no credential. The
  importer response contains only `showId`, never episode metadata.

## R12 metadata refresh

`refresh::Refresher::new(pool, provider, SystemClock)` refreshes saved shows. R24
starts it once per API process with `Arc::new(refresher).run(shutdown)`; several
processes may run it concurrently. Each pass (every 60 seconds):

- Schedules a `metadata_refresh` job (`jobs`, 0006, payload `{"showId"}`, no user)
  for up to 100 saved shows not fetched within 24 hours, stalest first, unless the
  show has a queued/running job or one created within 24 hours. Scheduling holds a
  transaction advisory lock because `jobs` has no per-show uniqueness for this kind.
- Claims up to 4 jobs with `FOR UPDATE SKIP LOCKED` and a 2-minute lease, and
  refreshes them concurrently. A crashed worker's claim is reclaimed after its lease;
  a claim that already used all 6 attempts is marked failed instead.
- Fetches the validated snapshot outside any transaction, then applies it with the
  importer's `persist` under the same provider lock, and marks the job ready in the
  same transaction only while the claim is still held. A snapshot that collides
  (identity owned by another show, number conflicts at commit) rolls back entirely.
- Provider outage or invalid/incomplete snapshot: the job is requeued with jittered
  exponential backoff (1 minute doubling to 1 hour, plus up to 25%); after 6 claims
  it fails and the show is rescheduled after the next 24-hour interval.
- Provider 429 with Retry-After: the job is requeued for the cooldown and the worker
  claims nothing until it ends.
- Finished jobs are pruned after 7 days.

Episodes match by provider ID through renumbering; removed seasons/episodes are
archived; progress, library and history rows are never written or deleted. The 0002
triggers advance `catalog_revision` when membership, numbering, release data or
archival change, so catch-up previews built earlier become `PREVIEW_STALE`; an
unchanged snapshot keeps the revision. New episodes, dates and show status are
picked up, and progress state is derived on read (R13), so a caught-up show with a
newly released episode reads as in progress.

`refresh::metadata_stale(fetched_at, now)` is the C05 `metadataStale` rule: true when
the last successful fetch is older than 48 hours. R18 sets the Show DTO flag with it
using the server clock; the saved catalog stays served either way. `Clock` is
injectable for tests; production uses `SystemClock`. Logs contain only job IDs,
attempt counts and fixed categories (`provider_unavailable`, `invalid_snapshot`,
`rate_limited`, `database`).

## Attribution

The adapter exposes `ATTRIBUTION`, `ATTRIBUTION_URL` and `ATTRIBUTION_GUIDE` for D04.
R24/settings UI must display the required approved TMDB logo alongside the text and
link. This backend issue does not add a logo or UI screen.

Official references checked 2026-10-07: [TV search](https://developer.themoviedb.org/reference/search-tv),
[TV details](https://developer.themoviedb.org/reference/tv-series-details),
[season details](https://developer.themoviedb.org/reference/tv-season-details),
[image configuration](https://developer.themoviedb.org/docs/image-basics), and
[attribution guidance](https://developer.themoviedb.org/docs/faq).
