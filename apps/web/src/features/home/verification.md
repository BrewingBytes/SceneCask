# R23 verification — 2026-10-12

Prerequisite R21 (#21) is closed and merged into `main` (#93). Changes stay in
`features/home/**`, `app/home/**` and `app/page.tsx`, plus two small shared edits (below).

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/home/playwright.config.ts` | Pass: 15 Chromium tests |
| Same suite with the focus/visibility refetch removed | The new-release refetch test fails |
| Library, show, Discover, episode and auth suites (after the shared fake change) | Pass: 19, 17, 20, 22, 22 |
| `node apps/web/src/features/home/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline 9 clones; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/` and `/home` are static shells, with account data fetched by the client |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test` with local `.env` (PostgreSQL) | Pass: 228 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 tests, after updating the foundation smoke heading (below). Not R24 integration evidence |

Acceptance criteria (fixture API at the `/api/v1` boundary):

- **E1/E3 marked → featured next is E2.** Featured shows Hollow Orchard, `S1 E2`, "2 of 4
  episodes watched · some out of order", "Title hidden until you watch", and Mark S1 E2 watched.
  No episode read runs and no protected title reaches the page.
- **New release after caught up → next unwatched on refetch.** Night Shift waits on "S2 E3 ·
  Airs Nov 12, 2026 (estimated)". The fake then releases S2 E3; a focus event refetches `/home`,
  and Night Shift becomes featured with Mark S2 E3 watched.
- **No friends or library → solo journey.** An empty library shows "Nothing to track yet" with
  Find a show → `/discover` and no social area. Saved shows with none Watching show "Choose a
  show to watch" → `/library`.
- **Mark/Undo.** A mark sends PUT with an Idempotency-Key and CSRF token, and Home refetches
  (featured moves to S1 E4). Undo restores S1 E2. A 503 mark keeps Home, and Retry reuses the
  same key. Marking a show's last released episode moves it to Waiting and moves focus to the
  "Up next" heading.
- **Catch-up.** The featured link opens the shared sheet ("Marks 2 episodes watched: S1 E2,
  E4."). Commit sends only the preview, and Home refetches.
- **States.** Load error with Retry, signed out and unverified use product copy, never server
  messages. Welcome links to Create account and Sign in, and a signed-in session goes to `/home`.
- **Layout.** At 320/390/860/1440px for Home, all caught up, empty and Welcome: no horizontal
  overflow, every button and link at least 44px, and axe (WCAG 2.1 AA) clean. Loading and error
  were captured at 390/1440.

## Shared edits (owner sign-off needed)

- `features/tracking/fixture-api.ts` (test-only): `FakeTrackingApi` takes an optional show
  identity, so the Home suite can serve several shows. The default stays the fixture show, and
  the other suites pass unchanged.
- `tests/e2e/foundation.spec.ts` (R01): the smoke test expected the old placeholder heading on
  `/`. This issue replaces that page with Welcome, so the assertion now expects the Welcome
  heading.

## Manual evidence and limitations

- I reviewed the screens through Playwright screenshots (`verification/`), not in a separate
  hand-run browser session.
- These tests use a fixture API, not live integration (R24 owns that gate).
- `GET /home` carries no release dates, so caught-up notes read the show and its latest season
  per caught-up show (two reads each). A dedicated field would be a contract change for R03.
- The shell's Home tab still links to `/` on other routes. Welcome forwards a signed-in session
  to `/home`; switching `homeHref` app-wide is R24's integration work.
- The page mounts its own ToastViewport until a root one exists. Registering this suite in CI is
  R01-owned.
