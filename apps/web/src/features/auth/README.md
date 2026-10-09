# Account screens (R10, C04)

Account, recovery and Google sign-in screens, plus profile onboarding. All calls go
through the generated client (`api.ts`). The adapter keeps only the session's CSRF token,
in memory. No credential, token or session value is written to storage or the URL.

| Route | Component | What it covers |
| --- | --- | --- |
| `/auth/signin`, `/auth/signup` | `CredentialsForm` | Google button above email and password, separated by "or". Field and form errors, busy state, generic wrong-password message. `?returnTo=` (allowlisted), `?reason=expired` and the Google callback's `?error=` |
| `/auth/verify` | `VerifyEmail` | From the email link (`#token=`): exchanges the token once, then onboarding. Otherwise "check your inbox", resend with cooldown, and expired/replayed recovery |
| `/auth/reset`, `/auth/reset/confirm` | `ResetRequest`, `ResetConfirm` | Enumeration-safe request, resend. New password from the link, success. Expired or incomplete link offers a new link, Google and sign-in |
| `/onboarding/profile` | `ProfileOnboarding` | Display name and permanent handle with an initials preview. Inline taken/format errors. Save or Skip go to `/discover` |
| (Settings, D04) | `ReauthDialog` | Password or Continue with Google (`intent=reauth`). Resumes the caller's pending action |

- **Sessions start and end with full document loads.** `enterSession` (after sign-in) and
  verification use `location.assign`/`replace`, so nothing the previous page held in memory
  survives. `endExpiredSession()` is the 401 handler for private screens: it *replaces* the
  current entry with `/auth/signin?reason=expired&returnTo=…`, so Back can't bring the old
  screen back either. R24 wires it into the shared data layer.
- **Email-link tokens** arrive in the fragment (the API puts them there), which never reaches
  a server or a Referer. `useFragmentToken` reads the token once and scrubs it from the address bar
  and history. It also handles a link pasted into a tab already on that page, which is
  a fragment-only navigation with no reload.
- **Google** is a browser navigation to `/api/v1/auth/google/start`, never fetch. The button
  stays busy while the redirect is pending. Callback failures land on sign-in with a fixed code,
  and the screen shows product copy with "Try Google again" and "Use email instead".
  `link_required` gets a dedicated explanation: sign in the existing way, then link from
  Settings. Accounts are never merged.
- **`returnTo`** is checked by `safeReturnTo`, which mirrors the API's Google allowlist. That
  means application paths only and never auth pages, with encoded, escaped or
  protocol-relative forms refused.
- Copy comes from `copy.ts` and `validation.ts`, never from server messages. Only the
  *names* of rejected fields are read from validation errors.
- Auth pages set `referrer: no-referrer` and load no third-party resources (the Google mark is
  inline SVG).

## Checks

The fixture app renders the production routes on port 3120, plus stand-ins for `/home`,
`/discover`, a private `/library` and a Settings-like `/reauth`. Playwright supplies the
`/api/v1` responses at the network boundary. This isn't live integration: the API router
doesn't mount the auth modules yet, and R24 owns the live gate.

```sh
yarn playwright test --config apps/web/src/features/auth/playwright.config.ts
node apps/web/src/features/auth/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
