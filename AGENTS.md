# SceneCask implementation guidance

Read docs/rebuild/decisions.md, architecture.md and contracts.md before changes.
Approved contracts override prototype simulations. Never import design support.js.

Ownership: R01 owns root manifests/lockfiles, web/API scaffolds, root scripts,
Compose and CI. R02 owns migration sequencing (0001–0007). R03 owns
contracts/openapi.yaml, generated client and generator configuration. R04/R05 own
shared UI primitives. R24/R38 own application integration and shared routers.
Feature owners work within their modules. Coordinate shared changes with their
owner; do not revert unrelated work. Do not invent migrations or domain routes.
Generated client code is never edited manually.

Use pnpm and the pinned Rust toolchain. From a configured checkout run:
- pnpm lint (web lint, Rust format and clippy)
- pnpm typecheck
- pnpm test (requires real PostgreSQL via DATABASE_URL)
- pnpm build
- pnpm api:check; pnpm api:generate when changing API contracts
- pnpm test:e2e (real API/PostgreSQL; browser installed)

R01's API generation commands report that R03 is pending until the schema exists.
Once present, they require R03's packages/api-client api:generate/api:check scripts.
Do not report this pending step as a validated API schema.

Never log configuration values, credentials, protected content or provider payloads.
Errors must omit protected strings and image paths. UI features require D09 browser
verification. Record actual automated/manual evidence and remaining limitations.
