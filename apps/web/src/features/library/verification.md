# R20 verification — 2026-10-12

Prerequisites R05 (#5), R15 (#12) and R18 (#17) are closed and merged into `main`. Changes
stay in `features/library/**` and `app/library/**`, plus two shared extractions in
`features/tracking/**` (below).

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/library/playwright.config.ts` | Pass: 16 Chromium tests |
| Same suite with Idempotency-Key reuse removed | The unknown-outcome remove test fails |
| Same suite with the single-row recovery read removed | The rejected status-change test fails |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/show/playwright.config.ts` | Pass: 17 tests (after the shared extraction) |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/search/playwright.config.ts` | Pass: 20 tests |
| `node apps/web/src/features/library/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline 9 clones; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/library` is a dynamic route |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test` with local `.env` (PostgreSQL 18) | Pass: 228 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 existing tests. Not R20 integration evidence |

Acceptance criteria:

- **Empty library → Discover.** With no saved shows, "Your library is empty" shows without
  search or filters, and "Find a show" navigates to `/discover`.
- **No matches keep filters.** `/library?status=dropped&q=orchard` shows "No Dropped shows
  match “orchard”." with the search text and Dropped filter still applied and All 4 still
  counted. "Clear search and filter" resets both, focuses the search field and clears the URL.
- **Canceled incomplete show.** Lantern Coast (`state: caught_up`,
  `releaseInfoIncomplete: true`) shows "Caught up" and "release info incomplete", never
  "Completed". Night Shift (`state: completed`) shows Completed. There is no Completed filter.
- **Failed status/remove recovers without overwriting later edits.** Another device puts
  Hollow Orchard on hold; this page's move to Dropped is held while Paper Moons moves to
  Watching and succeeds. The 409 shows product copy (no server message), and a single
  `GET /shows/{id}` restores Hollow Orchard to On hold. Paper Moons stays Watching on screen
  and on the server, and the counts match. Retry sends `expectedRevision: 2` with a new key.
  A remove that gets a 503 rolls back only that row; Retry sends the same key and body.

Also covered: status filter and debounced search hit the API once, stay in the URL and
survive reload; optimistic status change and recount before the response, then Undo;
a status change under a filter moves the row out of view; removal keeps status and
history, and Undo restores the row; keyboard-only filters and row menu; load error with
Retry, signed-out state with `returnTo=/library`; cursor pagination with focus on the
first new row.

## Browser checks

At 320, 390, 860 and 1440px: no horizontal overflow on the list, open row menu, no-match
and empty states; every visible button/link in the page is at least 44px tall; axe
(WCAG 2.0 A/AA, 2.1 AA) reports no violations, including the error state. The nav
switches from the bottom bar to the header at 860px. Tests run with reduced motion.

Screenshots (390 and 1440) in `verification/`: library, no-match, empty, loading, error.
Manual review was done through these Playwright screenshots, not a separate hand-run
browser session.

## Shared changes (need R21 owner sign-off)

The duplication ratchet flagged code repeated from the tracking client and the show-detail
suite, so it was moved into shared helpers rather than copied:

- `features/tracking/mutation-support.ts`: per-scope Idempotency-Keys and the toast Undo
  flow, now used by `tracking-client.ts` and the library client. Behavior is unchanged; the
  show-detail suite passes.
- `features/tracking/fixture-api.ts`: test-only fake-API plumbing (session, request log,
  idempotent replay, JSON/error helpers), now used by `show.spec.ts` and `library.spec.ts`.

## Limitations

- Tests run against a fixture API, not live integration (R24 owns that gate).
- `/shows/[showId]` and `/discover` are stubbed in the fixture app as navigation targets.
- The page mounts its own ToastViewport until the root one exists.
- After a list reload, a row changed outside the loaded pages triggers a full reload so
  counts stay correct. Counts can drift by an external change made between loading two
  pages, until the next reload.
- Registering this suite in CI is R01-owned.
