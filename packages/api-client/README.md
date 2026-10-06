# SceneCask API client

`contracts/openapi.yaml` is the canonical OpenAPI 3.1 source for all 59 C03–C08 operations.
Change it first, then run `yarn api:generate`. Never edit `src/generated/schema.ts`
or `contracts/operation-issues.json` manually. The operation mapping is derived from
each operation's `x-issue` and `x-rebuild-owner` (issue numbers differ from rebuild IDs).

This workspace pins openapi-typescript 7.13.0, openapi-fetch 0.17.0, Redocly CLI
2.53.3, AJV 8.20.0 and TypeScript 5.9.3. TypeScript is isolated here to match the
generator's supported peer range; the web workspace retains its foundation toolchain.
The only shared dependency change is the lockfile required to register/install this
workspace. Root scripts and CI already delegate `yarn api:check` to this package,
so their existing contract gate now runs validation, tests, compilation and drift checks.

```ts
import { createSceneCaskClient } from "@scenecask/api-client";

const api = createSceneCaskClient();
const { data, error } = await api.GET("/episodes/{id}", {
  params: { path: { id: episodeId } },
});
if (data && data.detailsAccess !== "locked") {
  // Narrowing is required: locked episodes have no details property.
  renderDetails(data.details);
}
```

Create a client per viewer/request. Browser cookies use same-origin credentials;
SSR callers provide an absolute same-origin `baseUrl` and explicitly forward only
the current viewer's cookie through `headers`. Responses use `cache: "no-store"`.
Do not share a client with viewer cookies across SSR requests or shared caches.
The browser supplies Origin; use the configured public origin in the typed header
parameters. Session-paired `X-CSRF-Token` comes from `/session`, never a global
server variable. Progress/library/catch-up/history/Undo headers also require a UUID
`Idempotency-Key`; retry unknown outcomes with the same key. Bodyless mutations
still declare JSON content type. Google start/callback are browser navigations with
303 redirects; do not initiate Google sign-in through an AJAX redirect to Google.

`Episode` is a closed union of locked, watched and revealed variants. Locked payloads
omit all protected keys. `Comment` distinguishes live text from bodyless tombstones;
`VisibleLibraryItem` omits internal revisions. Mutation requests reject unknown keys.
Error field messages are typed strings; runtime handlers must still enforce safe
messages and privacy policy. Types/schema validation cannot prove runtime redaction.

C08 specifies export categories without an example wire envelope. `AccountExport`
names those categories (`profile`, `library`, `progress`, `comments`, `follows`,
`blocks`, `notifications`) with explicit typed fields and no credential/path fields.
This supplies the required concrete download DTO without changing export scope.
Release state uses `released`, `future`, `unknown`; undated releases use null date.
These wire details follow the canonical behavior; there are no product contract changes.

Run `yarn api:check` from the repository root. Its nine automated tests cover complete
endpoint ownership, every schema/request/success/error example, forbidden spoiler
fields, tombstones, mutation validation, security/pagination/caching declarations,
deterministic generation and stale generated output, mutation transport, and isolated
viewer cookies. The generated header fingerprints the entire parsed contract, so
example/description changes also require regeneration. Generation compares in memory
and never repairs drift during check mode.

Verification for issue #3 (2026-10-06):

| Automated check | Result |
| --- | --- |
| `yarn api:generate` | Generated schema and operation mapping |
| `yarn api:check` | OpenAPI lint, 9 tests, strict client compile, drift check pass |
| `yarn lint` | Web lint, pinned Rust format/clippy pass |
| `yarn typecheck` | Web compile pass |
| `yarn build` | Web production build and pinned Rust build pass |
| `yarn test` with local environment loaded | 12 tests pass against real PostgreSQL/Mailpit |
| `yarn test:e2e` with local environment loaded | 6 existing foundation browser tests pass |

Local environment was loaded with Node's `--env-file=.env`, without printing values.
The sandboxed first Rust test attempt could not connect to local services; the rerun
with local network access passed. Existing browser evidence is in
`test-results/foundation-390.png` and `test-results/foundation-1440.png`; this issue
changes no UI and does not claim feature browser validation or new manual screenshots.
Client transport tests use a synthetic fetch boundary, not live domain handlers.

There are no new runtime handlers, migration changes or frontend feature integration.
R24/R38 remain the live integration gates; provider smoke checks/deployment still need
Google/TMDB/SMTP credentials and a domain/TLS. No real-provider calls were made here.
Once this PR merges, R06 and R11 can start (their R02 prerequisite is already merged).
Other backend owners can implement their mapped contract operations after their own
prerequisites are satisfied.
