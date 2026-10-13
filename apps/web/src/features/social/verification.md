# R28 verification — 2026-10-13

The prerequisite R27 (#28) is closed and merged into `main` (#109). Changes stay in
`features/social/**`, `app/friends/**` and `app/people/**`. No shared files, contracts or
migrations changed.

## Automated evidence

| Command | Result |
| --- | --- |
| `yarn playwright test --config apps/web/src/features/social/playwright.config.ts` | Pass: 17 Chromium tests (12 behavior tests, plus responsive checks at 320, 390, 859, 860 and 1440) |
| `node apps/web/src/features/social/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline: 36 clones, 314 lines; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/friends` and `/people/[id]` are dynamic routes |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test`, `yarn test:e2e` | Not run: no backend or e2e changes |

Acceptance criteria:

- **A private request persists as Requested until the server approves it.** Request to follow
  sends one `POST /people/{id}/follow` with the CSRF token. The row then says Requested, and focus
  moves to Cancel request. After 3 seconds both the row and the fake's server state still say
  pending. Only after the fake approves on the server does a fresh profile read show Following
  and the library. Cancel request sends `DELETE` and returns the row to Request to follow. When
  the response is lost after the write commits, the person is re-read and the pending request is
  announced.
- **A blocked or private viewer sees no hidden library counts or activity.** A private profile
  without access shows "@bea’s library is private". There are no episode counts, progress bars or
  show titles, and neither `GET /people/{id}/library` nor `GET /activity` is requested. That stays
  true after sending a request. A library 404 (access lost between the two reads) shows the same
  locked state. After a block, there is no Undo; a reload shows "This person isn’t available" with
  no name, and search no longer finds them.
- **With no friends, search is available and Home stays useful.** Friends opens on Find people
  with "Find people you know". The empty activity slot says "Follow people to see what they’re
  watching. Your tracking works the same without them." Both empty slots link to Find people.
  Home renders without the slot (R23); R38 places it.

Also covered:

- **Search.** No request below 2 characters, with a hint. A leading `@` is dropped. Shows the
  visibility and relationship from the API, and a no-results state.
- **Public follow** is immediate. Unfollow is confirmed. Escape cancels with focus returned, and
  after confirming, focus moves to Follow. A 429 keeps the server's state and shows an alert.
- **Without a handle.** The page shows a notice with "Choose a handle". A 422 `fields.handle`
  alert links to `/onboarding/profile`, and the follow isn't applied.
- **Requests.** `?tab=requests` deep link with a count. Approve fails with a 503 and the row is
  kept. Approve then succeeds: the row is removed, the heading takes focus, and Followers
  refreshes. Decline says they aren't notified. A 404 says "no longer pending". The tab is kept in
  the URL.
- **Followers.** Remove is confirmed. A 503 failure stays in the dialog. On success the row is
  removed and the heading takes focus.
- **Feed slots.** Activity shows the actor, show title and episode code, with a UTC timestamp.
  Suggestions say "Saved by Dee Park and Fay Moreno". Both slots are read-only. An error offers
  Retry.
- **Session.** An expired session goes to sign-in with `returnTo=/friends?tab=requests`.

## Browser checks

Automated in the responsive test at each width: no horizontal overflow (320 included), a column of
at most 760px inside 20px gutters, desktop navigation at 860, and 44px targets for buttons,
fields and standalone links. Inline links inside sentences are exempt under WCAG 2.5.8. axe
reports no WCAG 2.0 A/AA or 2.1 AA violations at 390 and 1440. The suite emulates reduced
motion. Keyboard focus through dialogs, rows that are removed and buttons that change state is
asserted in the behavior tests.

Screenshots (`verification/`, 390 and 1440): find-people, requests, profile-locked,
profile-library, block-confirm, feed, followers-error.

Manual: I reviewed the screenshots above against D08 and the prototype's Friends screen. No live
API or browser session was run by hand.

## Limitations

- Mock-only: the fake answers at the network boundary. The live relationship matrix (private and
  public, block, revoked access) is R38's gate.
- Beta off is not exercised by this suite: the fixture server always sets the flag. The routes
  call `notFound()` when it is off.
- `proxy.ts` (R24/R38) doesn't include `/friends` or `/people/*`. The screens handle a 401 instead.
- The Home and Discover slots are exported but not wired into those routes (R38).
