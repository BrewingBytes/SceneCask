# R01 verification — 2026-10-06

Scope: issue #1/C01. No prerequisites; no domain routes, schema or migrations.

Changed areas: root pinned manifests/lockfiles/toolchains, apps/web App Router
scaffold and same-origin proxy, services/api Axum/SQLx health and SMTP transport
boundary, Compose, root scripts, Playwright foundation tests, CI, README/AGENTS.
Next.js creates the committed apps/web/AGENTS.md and CLAUDE.md guidance.

## Automated results (local macOS, Node 22.22.0, pnpm 10.18.3, Rust 1.98.0)

- `pnpm lint`: passed web ESLint, Rust fmt, clippy all targets with warnings denied.
- `pnpm typecheck`: passed Next route generation and TypeScript.
- `pnpm test`: passed six Rust tests using PostgreSQL 17.6 and Mailpit 1.27.8.
  Includes successful SELECT 1 readiness, closed/unreachable DB 503, liveness
  independent of DB, missing/invalid configuration startup errors without values,
  real SMTP delivery and redacted delivery failure.
- `pnpm build`: passed Next production build and locked Rust build.
- `pnpm api:generate` / `pnpm api:check`: executed; explicitly pending R03.
  There is no canonical schema yet; this is not API generation/drift evidence.
- `pnpm test:e2e`: six Chromium tests passed against actual web/API/PostgreSQL.
  Same-origin health endpoints plus 320/390/859/860/1440 layout checks.
- `git diff --check`: passed.

## Clean checkout setup and smoke

Cloned the issue branch to /tmp/scenecask-r01-clean without application artifacts.
The documented setup passed using pinned toolchains and existing dependency caches:

```sh
pnpm install --frozen-lockfile
rustup show active-toolchain
cargo build --locked --manifest-path services/api/Cargo.toml
cp .env.example .env
pnpm dev:services
pnpm dev
# Separate terminal:
pnpm health:smoke
```

Both Compose services were healthy. Both applications started. Through web port
3000, /health/live returned 200 live and /health/ready returned 200 ready.
No existing build artifacts were copied. Compose reused the dedicated development
volume; readiness does not rely on application schema. No production credentials.

## Manual observations

Mailpit API confirmed one captured fixture message after the Rust mail test.
390px and 1440px screenshots were visually inspected; retained alongside this
report. The framework dev indicator is visible in development screenshots.
The static foundation page has no focusable controls, personalized data,
loading/error/empty flows or motion; feature keyboard/state checks belong to
subsequent UI work. Typography uses system fallbacks until R04 owns bundled fonts.

## Handoff and limits

No contract deviations. SMTP capture uses dedicated localhost ports 10259/8029
because the usual SMTP port was occupied. PostgreSQL uses localhost:54329.
API generation awaits R03's schema and packages/api-client api:generate/api:check
scripts; the wrapper fails if a schema is present without a configured generator.
Migration sequencing remains R02's responsibility.

The CI workflow repeats checks using PostgreSQL/Mailpit fixtures on a clean runner.
Local results are separate from hosted CI results; consult the PR checks for those.
No production deployment was attempted. Deployment inputs: database, SMTP/TLS and
credentials, Google/TMDB access, domain/TLS and same-origin reverse proxy.

R02 (#2), R03 (#3), and R04 (#4) can begin after this PR is merged.
