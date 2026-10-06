# Architecture decision record — rebuild

Status: stack and provider approved. Canonical implementation contracts are in contracts.md; missing screens are specified in design/supplement.md.

## Approved stack

Next.js/TypeScript, Axum/Rust, PostgreSQL/SQLx, REST/OpenAPI with generated TypeScript client, server-managed cookie sessions. Target: BrewingBytes/SceneCask (public, no default branch returned by GitHub at intake).

## Implementation conventions

- `apps/web/src/app/`: Next.js routes and composition. Feature logic belongs in `apps/web/src/features/{auth,catalog,library,tracking,social,discussions,settings}/`.
- `apps/web/src/components/ui/`: shared accessible primitives; `apps/web/src/styles/`: tokens/global styles. Feature-specific components stay in their feature.
- `services/api/src/modules/{auth,catalog,library,tracking,social,discussions,moderation,notifications,account}/`: each module has `mod.rs`, `routes.rs`, `service.rs`, `repository.rs`, and `dto.rs` as needed. Do not create empty files solely to satisfy this convention.
- `services/api/src/domain/`: pure progress and spoiler policy; no HTTP or database dependencies.
- `services/api/src/{config,error,state}.rs`: shared wiring, edited by the foundation/integration owner.
- `services/api/migrations/`: SQLx migrations. Allocate monotonically increasing numbers centrally in issue definitions; do not let parallel agents select conflicting numbers.
- `services/api/tests/`: actual PostgreSQL integration tests and permission/redaction tests.
- `contracts/openapi.yaml`: canonical API source, with schema validation and generated-client drift checks in CI. Rust handlers must conform to it.
- `packages/api-client/`: generated client and types; no manual changes to generated code.
- `tests/e2e/`: browser journeys against the real web/API/database stack. Controlled metadata/email fixtures are acceptable; browser-only mocks do not prove integration.
- `docs/rebuild/`: decisions, design references, contract explanations, ownership and coverage maps.

One issue owner coordinates shared OpenAPI changes and migration numbering. Feature issues own separate modules. Shared app/API routers, manifests, lockfiles and CI configuration require explicit integration ownership; do not schedule overlapping changes in one parallel group.

## Request and security boundary

Use one public origin with `/api/v1` routed to Axum. Axum owns the account/session and all authorization and spoiler filtering. Next.js never connects directly to PostgreSQL or metadata-provider credentials. Personalized responses and rendered pages must not enter shared caches.

Use opaque random session identifiers, store hashes server-side, and issue Secure/HttpOnly/SameSite cookies in production. Mutations require origin/CSRF protection. Define rotation, expiry, logout and account revocation in the auth contract. Email/password with verification/reset plus SSO is approved; Google is the initial SSO provider. Both sign-in paths establish the same server-managed session.

Keep external identities separate from accounts, uniquely keyed by issuer/provider and subject. Do not silently merge accounts because email strings match. Linking requires an authenticated account and proof of control of the external identity; unlinking must not remove the last usable sign-in method.

Privacy authorization runs before spoiler policy. A reveal grant can only relax spoiler protection for its named resource, session and user. It cannot grant discussion membership. Recheck access on every read/write, including after unfollow, follower removal, account visibility changes, and blocking.

An unauthorized payload must omit protected strings and asset paths entirely. Verify JSON, HTML/React server payloads, previews, accessibility names, search results, activity, notifications, prefetches and cache reuse. Do not build protection on CSS blur.

For progress writes, use transactions, idempotency keys and per-field revision provenance. Catch-up must confirm a specific set of eligible episode IDs; a changed metadata/progress revision requires a new preview instead of silently widening the set. Undo affects only fields still owned by the original action and reports skipped conflicts.

## Metadata provider: TMDB

Official references checked 2026-10-06:

- [TV search](https://developer.themoviedb.org/reference/search-tv)
- [Season details](https://developer.themoviedb.org/reference/tv-season-details)
- [Image URL configuration](https://developer.themoviedb.org/docs/image-basics)
- [FAQ and attribution/licensing](https://developer.themoviedb.org/docs/faq)

These cover the required search, season and image integration shape. No authenticated API probe has been performed. Provider selection is approved. The FAQ requires attribution for noncommercial use and directs commercial projects to obtain a commercial license; it offers no SLA. Do not assume an intended business model or license eligibility.

Keep provider IDs unique and separate from application IDs. Import metadata on the backend, retain episode identity through numbering corrections, and never erase watch history when provider data is missing. Use bounded timeouts, retry/backoff, cached last-known metadata, and explicit stale/unavailable states. Use UTC midnight for date-only releases without reliable timezone information and disclose uncertainty. Individual watched writes accept future and undated episodes; bulk catch-up and next-episode selection exclude them. A user's watched mark does not change the catalog release state.

## Remaining readiness gates

- Transcribe and validate the settled contracts in R03; implement the authorized design supplement.
- Supply deployment credentials for Google, TMDB and SMTP, domain/TLS and a database; no services are purchased or provisioned by backlog creation.
- Publish a versioned design snapshot accessible from issue links; local paths alone are insufficient.
- Finish OpenAPI, schema ownership, error envelope, pagination, request examples and policy truth tables before posting implementation issues.
