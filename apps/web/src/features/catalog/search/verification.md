# R19 verification — 2026-10-09

Prerequisites R05 (#59), R11 (#62), R15 (#77) and R16 (#79) are merged and are
ancestors of this branch. Changes stay in `apps/web/src/features/catalog/search/**`
and `apps/web/src/app/discover/**`, plus a two-line R03 client fix (below).

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/search/playwright.config.ts` | Pass: 19 Chromium tests |
| Same suite with the abort/stale guard removed, and with save-key reuse removed | The race test and the unknown-outcome test each fail; both pass restored |
| `node apps/web/src/features/catalog/search/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline 9 clones; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/discover` is a dynamic route |
| `yarn test:design` | Pass |
| `yarn api:check` | Pass: 20 client tests, strict and web-consumer compile, no drift |
| `yarn test` with local `.env` (PostgreSQL 18) | Pass: 189 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 existing tests. Not R19 integration evidence |

What the suite proves (acceptance criteria):

- **Slow earlier query never overwrites a later one.** "hollow" answers after 1.5s
  with a result that must not appear; "hollow orchard" answers immediately. Only the
  two settled terms are requested (debounced keystrokes), the earlier request is
  aborted, the stale title never renders after 1.8s, and the URL holds `?q=hollow+orchard`.
- **Ambiguous title.** Two "Hollow Orchard" rows differ by year, genres and poster.
  Adding 2011 imports provider 2011, then `PUT /library/{2011 app ID}` with
  `{saved:true,status:"plan_to_watch",expectedRevision:0}`, a UUID Idempotency-Key and
  the session CSRF token. The 2024 row is untouched. Opening 2024 imports it and
  navigates to `/shows/{app ID}`.
- **Failures keep the query and retry without duplicates.** Search 502 shows product
  copy (never the server message), Retry recovers. Import 502 sends no save; Retry
  saves exactly once. A reset connection on save is retried with the same key and
  body, reusing the import. 409 refetches the entry and retries with a new key and
  its revision (status preserved). An already-saved show sends no save. Repeated
  clicks/Enter while busy start one import and one save.
- Undo sends the action's undo with a UUID key and restores Add. Pagination failure
  keeps loaded results; Retry appends, de-duplicates and focuses the first new result.
  Empty, short-query, no-results and signed-out states. Keyboard order: search, each
  result's Open then Add; focus stays on Add as it becomes "In library".
- **Responsive (320/390/859/860/1440).** No horizontal overflow, controls ≥44px,
  bottom navigation below 860px and header navigation from 860px. axe (WCAG 2 A/AA,
  2.1 AA) reports no violations at 390 and 1440 for results, add failure and search failure.

Screenshots in `verification/` (390 and 1440): empty, loading, results, added
(Undo toast), add-error, search-error.

## R03 client fix

The generated client imported `./generated/security.js`. Turbopack (Next 16.3.8) does
not map that specifier to `security.ts`, so any web page using the client failed to
build. `packages/api-client` now imports `#generated/security` through a package
`imports` map. NodeNext type checking, the emitted-JavaScript tests and the web build
all resolve it. This needs R03 sign-off.

## Limitations

- Fixture harness only: `/shows/search` and `/shows/import` are not mounted yet, and
  `GET /shows/{id}` is not implemented. R24 owns live integration evidence.
- Search results carry no library state; "In library" is known only after an Add.
- `ToastViewport` is mounted by the Discover page until the integration owner mounts
  one at the root.
- `@scenecask/api-client` resolves through the workspace's hoisted install; adding it to
  `apps/web/package.json` and registering this suite in root scripts/CI are R01-owned.
- Manual checks were done through Playwright and screenshot review, not a separate
  hand-run browser session. Reduced motion is emulated in every test.
