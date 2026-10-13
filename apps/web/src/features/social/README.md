# Friends, profiles and social discovery (R28, C06)

Beta only. `app/friends/page.tsx` and `app/people/[id]/page.tsx` read `SCENECASK_BETA` per
request and answer 404 when it is off, like the API's social routes. Everything shown comes from
the C06 endpoints through the generated client (`api.ts`) on the shared account transport, which
keeps only the session's CSRF token in memory.

| Surface | Endpoints | What it covers |
| --- | --- | --- |
| `/friends` Find people | `GET /people`, `POST`/`DELETE /people/{id}/follow`, `GET /people/{id}` (re-read) | Debounced search by name or @handle (2–100 characters; a leading `@` is dropped). Rows show initials, name, handle, visibility and relationship, plus Follow / Request to follow / Cancel request / Unfollow |
| `/friends?tab=requests` | `GET /me/follow-requests`, `POST /me/follow-requests/{id}` | Approve or Decline. The tab label counts what's loaded. A notification can open this tab directly |
| `/friends?tab=followers` | `GET /me/followers`, `DELETE /me/followers/{id}` | Remove, confirmed: they lose access and aren't notified |
| `/people/[id]` | `GET /people/{id}`, `GET /people/{id}/library`, follow endpoints, `PUT /me/blocks/{id}` | Profile header, follow actions, Block (confirmed, no Undo), visible library or a locked explanation |
| `FriendsActivity` | `GET /activity` | Home's beta `social` slot: "From people you follow" |
| `FriendRecommendations` | `GET /recommendations` | Discover's beta `recommendations` slot: "Saved by people you follow" |

- **Relationships change only when the API says so.** A private account stays Requested until its
  owner approves it on the server. Nothing approves on a timer, and nothing changes optimistically.
  If a follow or unfollow response is lost, the person is re-read before the page says what
  happened. A 404 means the person is gone for this viewer (deleted, or a block either way).
- **Private profiles.** The library is requested only when `canViewLibrary` is true. Otherwise
  the profile shows why it's hidden and what would change that. It shows no counts, progress or
  activity. If access ends between the two reads (a library 404), the profile shows the same
  locked state.
- **Block** is confirmed: follows and requests are removed both ways and the person isn't
  notified. There is no Undo; the page points to Settings (R29), where Unblock lives. Focus moves
  to the "You blocked …" heading.
- **Handles.** Social writes need a handle (the API answers 422 `fields.handle`). Friends shows
  a "Choose a handle" link to `/onboarding/profile` when there isn't one. A refused write shows
  the same link in its alert.
- **Copy.** The Friends lede explains the account's visibility. A second line says that
  discussions need mutual follows with the host, and that participants can see one another (D08).
  Activity shows show titles and episode codes only, never episode titles, stills or comments.
  Empty states point to Find people and say that tracking works without friends.
- **States.** Every list has a loading skeleton, an error with Retry, an empty state, and cursor
  "Show more" with its own failure. A 401 anywhere ends the session and signs in again to the same
  path. Successes are announced politely; failures are alerts that stay until the next attempt.
  When a follow button changes state, focus moves to the button that replaces it.

## Integration (R38)

Pass `<FriendsActivity />` as `HomeView`'s `social` prop and `<FriendRecommendations />` as
`DiscoverSearch`'s `recommendations` prop, only when beta is on. `proxy.ts` (R24/R38) doesn't list
`/friends` or `/people/*` yet. The screens handle a missing session themselves, but adding them
there would redirect before rendering.

## Checks

The fixture app renders the production `/friends` and `/people/[id]` routes on port 3128 with the
beta flag on, and `/feed` renders the two exported slots. Playwright fakes the C06 endpoints at
the `/api/v1` boundary. This isn't live integration: R38 owns the live relationship matrix.

```sh
yarn playwright test --config apps/web/src/features/social/playwright.config.ts
node apps/web/src/features/social/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
