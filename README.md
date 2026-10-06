# SceneCask

A responsive personal TV tracker. This foundation contains a Next.js App Router
web application and Rust/Axum API; domain features will follow the rebuild backlog.

## Clean checkout

Prerequisites: Node.js **22.22.0**, Corepack, Rustup, Docker with Compose v2.
The root package pins pnpm **10.18.3**; rust-toolchain.toml pins Rust **1.98.0**
with rustfmt/clippy. Use `nvm install && nvm use` with the checked-in .nvmrc,
or configure your version manager from .node-version.

```sh
corepack enable
pnpm install --frozen-lockfile
rustup show active-toolchain
cargo build --locked --manifest-path services/api/Cargo.toml
cp .env.example .env
pnpm dev:services
pnpm dev
```

Open http://127.0.0.1:3000 and mail capture at http://127.0.0.1:8029.
PostgreSQL listens on localhost:54329; SMTP capture on localhost:10259.
Compose binds services only to loopback and retains PostgreSQL in a named volume.
`pnpm dev:down` stops containers without deleting the data. The dev supervisor
loads .env for both processes and shuts down the other process if either exits.
Next.js forwards /api/v1/* and /health/* to API_ORIGIN; there are no domain routes.

`DATABASE_URL` is required. Invalid configuration exits with a fixed message and
nonzero status without printing values. The API starts when PostgreSQL is offline:
`/health/live` returns 200 `{"status":"live"}`, while `/health/ready` performs
SELECT 1 with a three-second deadline and returns 503
`{"status":"database_unavailable"}` until PostgreSQL is available. Health responses
are not cached. SMTP delivery tests require SMTP_HOST/SMTP_PORT and send only fixture messages to
local capture. The mail boundary supports local capture and credentialed STARTTLS,
with bounded timeouts and fixed redacted delivery errors. No schema or migration is needed for the foundation health query.
The API defaults to 127.0.0.1:8080; API_BIND accepts an IP address and port.

## Verification

Run in a separate terminal after development services start:

```sh
set -a
. ./.env
set +a
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm api:generate
pnpm api:check
pnpm exec playwright install chromium
pnpm test:e2e
```

Stop `pnpm dev` before e2e: Playwright starts isolated API/web processes and requires
DATABASE_URL. `pnpm health:smoke` checks both endpoints through the running web
server; set SMOKE_ORIGIN to check another origin. E2e captures 390/1440 screenshots
and checks 320px overflow and the 860px gutter breakpoint. The foundation page is
static and has no interactive controls or loading/error states.

C01 also supports these direct Rust checks:

```sh
cargo fmt --manifest-path services/api/Cargo.toml --check
cargo clippy --manifest-path services/api/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path services/api/Cargo.toml
```

CI installs frozen lockfiles, uses real PostgreSQL and Mailpit, runs lint/typecheck/tests/build
and Chromium integration checks without Google or TMDB credentials.
R03 owns canonical OpenAPI and client generation. Until its schema is present,
api:generate/api:check explicitly report pending work; they do not validate a
schema. Once it exists they require packages/api-client's api:generate/api:check
scripts and propagate failures. R02 owns all migrations.

## Contribution conventions

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for
commit messages and PR titles: `<type>(<optional-scope>): <description>`.
Use an imperative description, such as `feat(auth): add email sign-in`.
Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`,
`chore`, `revert`. Use `!` and a `BREAKING CHANGE:` footer for breaking changes.

Name new branches `<type>/<issue-number>-<short-kebab-case-description>`,
for example `feat/6-cookie-sessions`. Omit the issue number when there is no
issue, for example `docs/contribution-guide`.

Use the [PR template](.github/pull_request_template.md) to describe implementation,
actual validation results, and the issue closed on merge.

## Deployment inputs

Production deployment is outside this issue. Supply a private PostgreSQL connection,
SMTP transport credentials/TLS, Google/TMDB credentials, and a domain/TLS reverse
proxy with one public origin. Configure API_ORIGIN before building Next.js and
API_BIND for the private runtime network. Never expose backend credentials with
NEXT_PUBLIC_* variables. Development credentials in .env.example and Compose are
for disposable local services only. No provider calls or account data ship here.
