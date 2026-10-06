# SceneCask rebuild decisions

Status: product and architecture decisions settled for backlog authoring. No application implementation.

## Approved by the user, 2026-10-06

- Rebuild from scratch; responsive web first; Rust backend; no video streaming.
- The user's message is the product authority. No separate approved specification was supplied. Design handoff labels `(C)` do not establish approval.
- Target repository: https://github.com/BrewingBytes/SceneCask.
- Next.js and TypeScript frontend; Rust with Axum backend; PostgreSQL with SQLx; REST with OpenAPI and a generated frontend client; server-managed cookie sessions.
- Do not consult, copy, reconcile, or modify existing GitHub issues.
- Track individual episodes, including out-of-order viewing. A single watched action never marks earlier episodes. Catch-up is explicit. Caught up differs from Completed.
- Backend filters protected content, including episode titles, images, discussions, activity, and notifications.
- Product must be useful without friends.
- Private accounts by default; approved followers see library/activity. Public accounts allow immediate follows. Each episode discussion belongs to a host; the host's mutual approved follows can participate and see one another.
- An individually watched episode unlocks its details. Discussion requires every regular episode through that episode watched. Separate session-scoped explicit reveals bypass only the spoiler gate for one episode or one discussion, never privacy permissions or progress. Specials discussions always require explicit reveal.
- Manual library statuses: Plan to watch, Watching, On hold, Dropped. Caught up and Completed are computed. Completed requires ended/canceled status, every regular episode watched, and no unresolved release dates.
- Regular episodes use season/episode order. Specials are excluded from next episode, normal progress, catch-up and completion.
- Future episodes CAN be marked watched individually (explicit user correction). Undated episodes can also be marked individually. Neither is silently included in bulk catch-up, which includes released regular episodes only. Future/undated episodes are not suggested as next unwatched.
- Release dates use UTC midnight when no reliable timezone is available, with uncertainty disclosed.
- Removing a show retains history; erasing history is a separate action. Undo reverses only that action's changes and preserves later edits.
- Authentication includes email/password with verification and password reset PLUS Google SSO.
- TV-only personal-tracking alpha, then social beta. Beta includes privacy settings, follower removal, blocking/reporting, a minimal operator report queue, in-app follow-request notifications, data export and account deletion. Reactions are deferred. No email/push social notifications.
- No playback, native apps, live watch parties, direct messaging, billing or AI recommendation infrastructure in this release scope.
- TMDB is the metadata provider. Include attribution and require appropriate API access before deployment.
- Specify missing settings/privacy, notifications, export/deletion, moderation and SSO screens using the supplied design system; do not wait for another Claude Design export.

## Engineering conventions adopted for implementation

- Monorepo: `apps/web`, `services/api`, `packages/api-client`, `contracts`, `docs`, and deployment configuration.
- Same-origin web/API deployment, with Axum owning authentication, authorization, sessions, spoiler filtering, and persistence.
- One canonical OpenAPI contract owner; generated client is never edited manually. One migration sequencing owner.
- Rust domain tests; PostgreSQL integration tests; API authorization and redaction tests; frontend component tests; browser journey/accessibility checks.

## Engineering choices

The module conventions, schema/API contract, design supplement and issue ownership map supply routine implementation details. Email uses a configurable SMTP transport and local capture in development; deployment uses container images and a same-origin reverse proxy, without selecting or purchasing a hosting vendor. Google/TMDB credentials, SMTP access, a deployment domain and appropriate provider access are deployment inputs, not reasons to leave implementation unspecified.

No further product decisions block backlog authoring. Implementation must follow this record where the prototype differs.
