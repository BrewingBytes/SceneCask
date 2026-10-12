# R24 alpha integration: verification

Recorded 2026-10-12 on macOS with local Compose PostgreSQL 18.6 and Mailpit 1.31.4.

## Automated

| Command | Result |
| --- | --- |
| `yarn lint` | Pass. Duplication unchanged (9 clones before and after, none in new files), web ESLint, rustfmt, clippy `-D warnings` |
| `yarn typecheck` | Pass |
| `yarn api:check` | Pass: generated client and operation mapping current (no contract change) |
| `yarn test` | Pass: 254 Rust tests, including the new `tests/application.rs` (3) and config/TMDB-fixture unit tests |
| `yarn build` | Pass. Private routes are dynamic (beta flag read per request); proxy guard built |
| `yarn test:e2e` | Pass: 15 tests (6 foundation, 9 alpha) on real Next.js + Axum + PostgreSQL + Mailpit, with TMDB/Google fixture servers at the provider boundary |

### `services/api/tests/application.rs` (production composition, real PostgreSQL)

- Every alpha route answers 401/403 when anonymous (mounted, guarded); beta paths answer 404 NOT_FOUND; anonymous logout is 204.
- Provider outage: search and import return 502 PROVIDER_UNAVAILABLE with no write. Save, out-of-order mark (E4 after E1 leaves E2 next), catch-up (E2/E3 only), a newer unmark/re-mark of E2, then Undo (reverted 1, skipped 1), logout (401) and a new session that reads the same state.
- Another session of the same user gets no reveal. Another member gets locked details, an empty library and 404 on Undo. An unverified account gets 403 EMAIL_UNVERIFIED with no catalog text. Logout removes reveal grants.

### `tests/e2e/alpha.spec.ts` (browser, live stack)

1. Fresh account, no friends: UI sign-up, real verification email via Mailpit, skip onboarding, search with disambiguation (2024 vs 2010), save, mark E3 (only E3 unlocks), Home Mark E1 makes E2 next, catch-up through E4 (exactly 2), a concurrent unmark from a second session, Undo ("1 change you made afterwards was kept"), reload, sign out (other tab leaves to `?reason=expired`, Back shows nothing), sign in to the same state. Every same-origin response body is scanned: no unwatched episode title is ever delivered.
2. Reveal E2 in one session. TMDB down: search error with Retry; saved show loads and marks persist across reload. The second session has no reveal. Another member's JSON, SSR HTML and prefetch payloads contain no protected title. An unverified account lands on verification, and its API reads are 401/403 with no payload.
3. Google sign-in through the fixture OIDC provider (discovery, PKCE, RS256 ID token) creates a new account, then onboarding.
4. Signed-out private route redirects to sign-in with `returnTo`; `/about` carries TMDB attribution.
5. 320/390/860/1440: Home, Discover, Library, Show, Episode and About have no horizontal overflow, the nav is a bottom bar below 860 and in the header from 860, and axe (wcag2a/aa, 21aa) reports no violations. Screenshots at 390/1440 are in this folder.
6. Keyboard: account menu Enter/arrows/Escape restores focus to the trigger; Sign out via keyboard.

## Manual

- Reviewed the 390/1440 screenshots: layout, gutters and locked titles ("Title hidden") are correct. The "N" bubble is the Next.js dev indicator (dev server only).
- Reduced motion was not separately re-checked here; primitives were verified in R04/R05.

## Limitations and deployment inputs

- No approved TMDB logo asset in the repo (the official asset URL returned 403); `/about` has the required text and link. Add the logo before deployment.
- Contract deviation: email-registered sessions return `displayName: null`, while OpenAPI `User.displayName` is a required non-empty string. The web onboarding and account menu tolerate null; the R03/R06 owners need to settle it (nullable schema or a server default).
- Behind the Next.js proxy, per-IP auth rate limits see the proxy address; forwarded-IP trust belongs to deployment (R39).
- Toasts remain mounted per feature page (one viewport per page), not at the root.
- Without Google credentials, `/me/identities*` is not mounted either (it needs the Google issuer); settings (R29) will need Google configured or a follow-up.
- Live Google/TMDB smoke checks are manual release evidence and were not run.
