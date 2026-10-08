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

Integration tests use `tests/common` helpers for app setup, signed requests, row
counts and error assertions; add a missing helper there instead of copying one
into a feature test file.

Use Yarn and the pinned Rust toolchain. From a configured checkout run:
- yarn lint (duplication check, web lint, Rust format and clippy)
- yarn typecheck
- yarn test (requires real PostgreSQL via DATABASE_URL)
- yarn build
- yarn api:check; yarn api:generate when changing API contracts
- yarn test:e2e (real API/PostgreSQL; browser installed)

R01's API generation commands report that R03 is pending until the schema exists.
Once present, they require R03's packages/api-client api:generate/api:check scripts.
Do not report this pending step as a validated API schema.

Never log configuration values, credentials, protected content or provider payloads.
Errors must omit protected strings and image paths. UI features require D09 browser
verification. Record actual automated/manual evidence and remaining limitations.
PRs that change the UI must attach mobile (390px) and desktop (1440px) screenshots
in the PR template's Screenshots section.

## Git conventions

Use Conventional Commits for commits and PR titles:
`<type>(<optional-scope>): <short imperative description>`.
Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert.
Examples: `feat(auth): add email sign-in`, `docs: clarify local setup`.
Mark breaking changes with `!` and explain them in a `BREAKING CHANGE:` footer.

Name new branches `<type>/<issue-number>-<short-kebab-case-description>`.
Use the same type vocabulary as commits. If no issue exists, omit the issue number.
Examples: `feat/6-cookie-sessions`, `docs/contribution-guide`.
Use the PR template, record actual validation, and link the issue with `Closes #N`.
