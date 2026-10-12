# R22 verification — 2026-10-12

Prerequisite R21 (#93) is merged and an ancestor of this branch; R18 (#90) supplies
`GET /episodes/{id}` and the reveal endpoints. Changes stay in `features/catalog/episode/**`
and `app/episodes/**`, plus a test-only extraction in `features/tracking/fixture-api.ts`
(below).

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/episode/playwright.config.ts` | Pass: 20 Chromium tests |
| Same suite with the synchronous eviction in Hide removed | The Hide-again test fails |
| Same suite with the pending-Hide redaction removed | The failed-Hide test fails |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/show/playwright.config.ts` | Pass: 17 tests (R21, after the shared fake extraction) |
| `CI=1 yarn playwright test --config apps/web/src/features/library/playwright.config.ts` | Pass: 19 tests (shares `fixture-api.ts`) |
| `node apps/web/src/features/catalog/episode/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication within the 9-clone baseline; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/episodes/[episodeId]` is a dynamic route |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test` with local `.env` (PostgreSQL) | Pass: 228 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 existing tests. Not R22 integration evidence |

Acceptance criteria:

- **Locked DTO.** The page markup, attributes, accessibility tree (`ariaSnapshot`) and browser
  title contain no protected title, overview or still path; no `<img>` exists and no still is
  requested at the network boundary. Server HTML and the RSC payload (`RSC: 1`) for a watched
  and a revealed episode contain no episode data at all (not even the show name); the HTML
  title is "Episode · SceneCask" with `no-referrer`. The client title is
  "Hollow Orchard S1 E3 · SceneCask". Headings and still alt text use the code only.
- **Reveal, then Hide again or sign-out.** Reveal opens an alertdialog (Cancel focused, Escape
  sends nothing). Confirm sends `POST /reveals {scope:"episode_details",resourceId}` with
  CSRF, then details appear only from the following `GET /episodes/{id}`; progress is
  unchanged. Hide again redacts the DOM while `DELETE /reveals/episode_details/{id}` is still
  held, then rereads a locked episode and announces "Details for S1 E3 are hidden again."
  A failed Hide stays redacted (even across a reread while the grant still exists) and
  offers Retry. After sign-out, a `pageshow` restore redacts before any request and shows the
  "Your sign-in session ended" state; a 401 from a write does the same; a lapsed reveal
  (`expiresAt`, clock fast-forwarded 12h) redacts and says so.
- **Future individual mark.** S1 E4 (future) sends one `PUT /progress/episodes/{id}`
  `{watched:true,expectedRevision:0}` with a UUID key; details appear only after the
  following `GET`; the line still reads "Airs Nov 12, 2026 (estimated)" and the
  not-released note remains, with the badge "Watched".
- Unmarking hides watched-unlocked details while the write is pending; Undo from the toast
  rereads and restores them. Undated and special notices render; specials have no catch-up
  link. "Mark watched through S1 E3…" opens the shared catch-up sheet previewed through this
  episode and commits only the previewed set. A rejected reveal keeps product copy in the
  dialog. Load failure Retry and not-found states never show server messages.
- **Keyboard.** Reveal → Tab → Enter lands focus on the revealed heading; Hide again moves
  focus to the redacted section, and Tab reaches Reveal.
- **Responsive (320/390/860/1440).** No horizontal overflow, controls ≥44px, axe (WCAG 2 A/AA,
  2.1 AA) clean for the locked page, reveal dialog, revealed page and session-ended state.
  Reduced motion is emulated in every test. The discussion slot renders and requests nothing.

Screenshots in `verification/` (390 and 1440): locked, reveal confirmation, revealed, session
ended, future episode.

## Shared test fake

R21's in-file C05 fake moved to `features/tracking/fixture-api.ts` as `FakeTrackingApi`
(test-only), so the episode suite extends it with reveal grants and sign-out instead of
copying it. R21's suite now uses it unchanged in behavior and passes. This touches R21's
test files and needs its owner's sign-off.

## Limitations

- Fixture harness only; R24 owns live integration and network-isolation evidence.
- No logout control exists yet. Sign-out is covered by session loss (401) and page restore;
  an in-app logout event for live tabs is for the integration owner to wire.
- `ToastViewport` is mounted by the page until the integration owner mounts one at the root.
- The page fetches client-side; server-rendered episode data with cookie forwarding is R24's.
- Discussions (beta) are a reserved, empty slot; R31+ owns them.
- Registering this suite in root scripts/CI is R01-owned.
- Manual checks were done through Playwright and screenshot review, not a separate
  hand-run browser session.
