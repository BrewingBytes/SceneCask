# Settings (R29, C04, C06)

`SettingsView` renders `/settings` (D04) in one 760px column. Everything shown comes from
the API (`GET /session`, `GET /me/identities` and the C06 lists); no setting is a local-only
toggle. All calls go through the generated client (`api.ts`) on the shared account transport
(`features/auth/transport.ts`), which keeps only the session's CSRF token in memory.

| Section | Endpoints | What it covers |
| --- | --- | --- |
| Profile | `PATCH /me` | Display name. The handle is read-only once chosen; an account without one chooses it here (taken/format errors inline) |
| Sign-in methods | `GET /me/identities`, `DELETE /me/identities/google`, `GET /auth/google/start?intent=link`, `POST /auth/password-reset-request`, `POST /auth/logout` | Link/unlink Google, set a password through an email link (SSO-only accounts), sign out |
| Privacy (beta) | `PATCH /me/privacy` | Private/Public radios. Public is confirmed with the D04 warning before any write |
| Followers (beta) | `GET /me/followers`, `DELETE /me/followers/{id}` | Remove with a confirmation stating loss of access and no notification |
| Blocked people (beta) | `GET /me/blocks`, `DELETE /me/blocks/{id}` | Unblock with a confirmation that follows don't come back |
| Your data (beta) | none | Link to `/settings/data` (D06, another issue) |
| About | none | TMDB attribution and a link to `/about` |

- **Beta.** `app/settings/page.tsx` reads `SCENECASK_BETA` per request (the flag that gates
  the API's social routes) and passes `beta`. With it off, the four beta sections aren't
  rendered and their endpoints are never called.
- **Fresh sign-in.** Linking and unlinking need a session reauthenticated within 5 minutes.
  Unlink tries first; a 401 opens `ReauthDialog`, and on success the pending unlink runs
  again. The rotated session's CSRF token is refreshed by one retry. Link always reauths
  with the password first: the API's link start answers 401 to a stale session, and it's a
  browser navigation, so the 401 can't be handled after the fact.
- **Google round trips** return to `/settings?google=link|unlink`, plus `&error=<code>` on
  failure. The page reads and scrubs these once. `link` announces success only if
  `GET /me/identities` says Google is linked. `unlink` reopens the unlink confirmation.
  Errors use `googleProblem` copy.
- **Last sign-in method.** With Google linked and no password, Unlink is `aria-disabled`
  with the explanation as its description. A 409 `LAST_SIGNIN_METHOD` (another device
  removed the password) shows the same explanation and re-reads the identities.
- **Privacy writes.** The radios show the server's visibility, or the choice being saved
  while the request is in flight (`aria-busy`; changes are ignored until it settles). A
  failed save returns them to the server's value with an alert and Try again.
- **Lists** have loading, error with Retry, empty, and cursor "Show more" states. A row
  disappears only after the API accepts. A 404 counts as already removed. Focus goes to
  the section heading.
- A 401 anywhere except unlink calls `endExpiredSession()`.

## Checks

The fixture app renders the production `/settings` route on port 3125 with the beta flag on,
and `/alpha/settings` renders the view with it off. Playwright fakes the C04/C06 endpoints
at the `/api/v1` boundary, and reauth rotates the CSRF token. This isn't live integration:
R38 owns the linking and live-privacy gate.

```sh
yarn playwright test --config apps/web/src/features/settings/account/playwright.config.ts
node apps/web/src/features/settings/account/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
