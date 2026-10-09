# R10 verification — 2026-10-10

Prerequisites R05 (#59), R09 (#81) and R15 (#77) are merged and are ancestors of this
branch. Changes stay in `apps/web/src/features/auth/**`, `apps/web/src/app/auth/**` and
`apps/web/src/app/onboarding/profile/**`. No shared files, contracts or migrations changed.

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/auth/playwright.config.ts` | Pass: 22 Chromium tests (Next dev, so React StrictMode double effects included) |
| Same suite with `endExpiredSession` using `assign` instead of `replace` | The 401 test fails (Back reaches the private entry); passes restored |
| Same suite with the Google button's busy guard removed | The Google test fails; passes restored |
| `node apps/web/src/features/auth/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at the 9-clone baseline; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/auth/signin` is dynamic, the other account routes are static |
| `yarn test:design` | Pass |
| `yarn api:check` | Pass; generated client current. No contract changes |
| `yarn test` with local `.env` (PostgreSQL 18) | Pass: 215 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 existing tests. Not R10 integration evidence |

### Acceptance criteria

- **New email user → real verification → Discover.** Sign-up posts `{email,password}`
  with the browser's Origin. The verify screen names the address and has no control that
  stands in for the link. Resend starts cooling for 60s, because sign-up just queued an email and
  the API skips a resend inside its cooldown. After the cooldown it posts once, then cools down
  again. Opening
  `/auth/verify#token=…` (in the same tab, which is a fragment-only navigation) posts
  `{token}` once and replaces the page with `/onboarding/profile`. Saving the profile sends
  `PATCH /me {displayName,handle}` with the rotated session CSRF token and lands on
  `/discover`. `localStorage` and `sessionStorage` are empty afterwards.
- **Canceled Google flow or expired reset → recoverable error and alternate method.**
  The Google start navigation carries `intent=signin&returnTo=/library`. While it's pending,
  repeated clicks and Enter start nothing more. A `?error=canceled` callback shows "Google
  sign-in was canceled" with "Try Google again" (starts a new flow) and "Use email instead"
  (focuses Email). The code is then removed from the address. Unknown codes read as "didn’t
  finish" and are never echoed. A 410 reset shows "This reset link has expired" with "Send a
  new link", Continue with Google and Sign in. A link without a token gets the same recovery.
- **401 during private navigation → sign-in without the previous viewer's data.** The
  stand-in private page shows A's data, then gets 401. It is replaced by
  `/auth/signin?reason=expired&returnTo=%2Flibrary` with a status notice. A's text and the
  page's in-memory cache are gone. Back goes to the entry *before* the private page. Signing
  in as B returns to `/library` with only B's data, and storage stays empty.

### Other behaviour covered

- Wrong password: one generic message with a Reset password link. Unverified correct
  credentials (403) lead to "Verify your email to sign in" without a session. A 429 on
  resend shows Retry-After copy and a matching cooldown.
- Field errors move focus to the first invalid field and are tied to it through
  `aria-describedby`. Server field messages (`SERVER-FIELD`) and error messages
  (`SERVER-MESSAGE`) never render. Passwords of 12 spaces are sent untrimmed. 503 and 429
  get product copy. Repeated submits send one request.
- Verify: an expired or replayed token shows recovery and the token leaves the address bar. A
  network failure offers Retry with the in-memory token.
- `returnTo` accepts application paths and drops fragments, and keeps the safe escapes that
  Discover's `?q=` carries (`/discover?q=grey%27s+anatomy`), as the API does. Protocol-relative,
  backslash, escapes decoding to separators or controls, non-ASCII, whitespace, control-character, auth-page, look-alike (`/homepage`) and over-long
  values fall back to `/home`. This is checked as a pure function and through sign-in.
- `link_required`: an explanation that accounts aren't merged, no Google button, and
  sign-in with the existing method.
- Reset: an enumeration-safe "If there’s an account for …" message, Resend cooling for the
  API's 60s cooldown, a mismatched
  confirmation is caught locally, and success says every session was signed out.
- Onboarding: malformed handles are caught locally, `HANDLE_TAKEN` is an inline error on
  Handle, Skip goes to `/discover`, an existing handle redirects to `/discover`, signed-out
  visitors go to sign-in with `returnTo`, and a failed session load offers Retry.
- Reauth dialog: focus starts on Password. A wrong password keeps the dialog open with an
  inline error. The right one closes it, resumes the action and returns focus to the trigger.
  Continue with Google uses `intent=reauth&returnTo=/settings`.
- Keyboard order on sign-in matches the visual order: skip link, brand, Google, Email,
  Password, Sign in, Forgot password, Create account.
- **Responsive (320/390/859/860/1440)** across nine states (sign-in, session expired,
  sign-up errors, Google canceled, link required, verify inbox, verify expired, reset link
  incomplete, onboarding with taken handle): no horizontal overflow, the column stays within
  480px and the 20px gutter, and every control in the column is at least 44×44px. axe
  (WCAG 2 A/AA, 2.1 AA) reports no violations at 390 and 1440 for every state.

Screenshots in `verification/` (390 and 1440) cover those nine states.

## Limitations

- Fixture harness only. The live API router mounts only health, so register, verify, login,
  reset, reauth, `PATCH /me` and Google start/callback aren't reachable from the web app
  yet. R24 owns live integration evidence. That includes a real email-link round trip through
  local mail capture and a real 303 from `/auth/google/start`.
- Account screens have no app navigation, so D09's 860px nav switch doesn't apply. The
  header is brand-only at every width.
- `endExpiredSession` is exported but no shared data layer calls it yet. Router and
  data-layer wiring is R24.
- `ReauthDialog` is verified on a fixture page. The Settings screen that uses it (and reads
  the reauth callback's `?error=`) belongs to D04's owner. `googleProblem` covers
  `reauth_mismatch` and `identity_in_use` copy for that screen.
- The Google button also has a same-task ref guard. Chromium collapses repeated
  navigations within one task, so the suite can't distinguish it; only the busy guard is
  proven by test.
- `@scenecask/api-client` resolves through the hoisted workspace install, as for R19.
  Adding it to `apps/web/package.json` and registering this suite in root scripts and CI are
  R01-owned.
- Manual review was screenshot review plus keyboard and axe checks run through
  Playwright. There was no separate hand-run browser session. Reduced motion is emulated
  in every test.
