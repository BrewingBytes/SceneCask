# R29 verification — 2026-10-13

Prerequisites R10 (#19) and R26 (#27) are closed and merged into `main`. Changes stay in
`features/settings/account/**` and `app/settings/page.tsx`, plus the shared extractions below.

## Automated evidence

| Command | Result |
| --- | --- |
| `yarn playwright test --config apps/web/src/features/settings/account/playwright.config.ts` | Pass: 22 Chromium tests |
| `yarn playwright test --config apps/web/src/features/auth/playwright.config.ts` | Pass: 22 tests (after the shared transport extraction) |
| `yarn playwright test --config apps/web/src/features/library/playwright.config.ts` | 18 pass, 1 fails: "a status change under a filter…" (two Undo toasts). It fails the same way on `main` without these changes |
| `node apps/web/src/features/settings/account/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline 9 clones; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/settings` is a dynamic route |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test`, `yarn test:e2e` | Not run: no backend or e2e changes |

Acceptance criteria:

- **Last sign-in method.** With Google linked and no password, Unlink Google is
  `aria-disabled` and described by "Google is the only way you sign in, so it can’t be
  unlinked. Set a password first, then unlink Google." Activating it opens nothing and sends
  no request. If the API answers 409 `LAST_SIGNIN_METHOD` (the password was removed
  elsewhere), the dialog shows the same explanation, re-reads the identities, and the
  button becomes disabled.
- **Public switch.** Choosing Public opens "Make your account public?" with the D04 warning.
  Nothing is written until Make public. Escape or Cancel keeps Private. While saving, the
  group is `aria-busy` and "Saving your privacy setting…" is announced. A 503 leaves Private
  checked, with an alert "Your account is still private. …" and Try again (which shows the
  warning again). Success comes from the response's `visibility`.
- **Unblock.** The confirmation reads "Follows between you were removed when you blocked them
  and won’t come back." Success announces "Dee Park is unblocked. Previous follows weren’t
  restored." Only `DELETE /me/blocks/{id}` is sent.

Also covered:

- **Profile.** Display name save with the fixed handle; choosing a handle when none is set,
  with a taken-handle error.
- **Unlink with a stale sign-in.** 401 → reauth (a wrong password stays open) → the unlink
  resumes on the rotated CSRF token, with one retry.
- **Google round trips.** Reauth (`?google=unlink` resumes the confirmation) and link
  (`?google=link`, with `?error=` copy), with the URL scrubbed.
- **SSO-only accounts.** Set a password through `password-reset-request`, including the
  429 copy.
- **Followers.** Remove with a failure that keeps the row, then success with focus on the
  heading. Loading, error with Retry, empty and cursor Show more states.
- **Beta off** (`/alpha/settings`): no social sections, and only `/session` and
  `/me/identities` are read.
- **Session.** An expired session goes to sign-in with `returnTo=/settings`; Sign out ends
  the session.
- **Load failure.** Retry, with no account data shown.
- **Keyboard.** Dialog focus is trapped, Escape cancels, and focus returns to the trigger.

## Browser checks

At 320, 390, 859, 860 and 1440px, for the page, last-method, public confirmation, failed
privacy save, unblock confirmation, and followers-error/blocks-empty states:

- No horizontal overflow.
- The column is at most 760px and starts at or after the 20px gutter.
- Every visible button, text input and radio choice is at least 44px tall.
- The header nav is visible from 860px.
- axe (WCAG 2.0 A/AA, 2.1 AA) reports no violations at 390 and 1440.

Tests run with reduced motion.

Screenshots at 390 and 1440 are in `verification/`: settings, last-method, public-confirm,
privacy-failed, unblock-confirm, empty-error. Manual review was done through these Playwright
screenshots, not a separate hand-run browser session. Full-page captures draw the fixed
mobile nav at the 900px viewport edge.

## Shared changes (need owner sign-off)

- `features/auth/transport.ts` (new) and `features/auth/api.ts` (R10): the generated-client
  transport shared by both adapters. It covers error-envelope mapping, `settle`, and the
  CSRF bootstrap with one retry after rotation. `createAuthApi`'s interface and behavior are
  unchanged; the auth suite passes.
- `features/tracking/fixture-api.ts` and `features/auth/auth.spec.ts` (test-only):
  `axeViolations`, `expectNoPageOverflow` and `responsiveTests` now live in the shared
  fixture module.

## Limitations and follow-ups

- **Not live integration.** Linking and live privacy are checked by R38. No live Google
  round trip was run.
- **No route into Settings yet.** The account menu (`app/_integration/account-menu.tsx`) has
  no Settings link, and `proxy.ts` doesn't guard `/settings`. Both are R24/R38-owned.
  Without the guard, the client sends signed-out visitors to sign-in after `GET /session`.
- **Cross-tab sign-out.** Sign out imports `announceSignedOut` from
  `app/_integration/session-channel.ts`, so other tabs leave too.
- **No TMDB logo.** About shows TMDB's attribution text and link, but no logo asset.
- **`/settings/data`** (D06) is linked when beta is on; another issue builds it.
