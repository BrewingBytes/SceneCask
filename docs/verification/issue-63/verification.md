# Issue #63 verification

Verified on 2026-10-08 with the pinned Node/Yarn and Rust toolchains, real local
PostgreSQL and SMTP capture from Compose. No application or API contract changes.

## Test suite size and coverage

The baseline is the unchanged Git HEAD before this refactor, exported to a temporary
checkout and tested with `cargo test --locked`. The updated checkout was tested with
`yarn test`. Both used real services and passed the same named tests.

| Rust test file | Before lines | After lines | Tests before / after |
| --- | ---: | ---: | ---: |
| catalog_import.rs | 785 | 678 | 8 / 8 |
| health.rs | 101 | 68 | 3 / 3 |
| mail.rs | 36 | 36 | 2 / 2 |
| schema.rs | 1180 | 1162 | 6 / 6 |
| session.rs | 918 | 780 | 14 / 14 |
| common/mod.rs | 0 | 278 | shared helpers |
| **Total integration Rust lines** | **3020** | **3002** | **33 / 33** |

Counts include the new common module; moving code is not counted as removing it.
The 19 in-crate tests also pass unchanged: **52 Rust tests before and after**, zero
failures or ignored tests. Fixture data is unchanged. The catalog authorization
matrix retains both anonymous cases and both unverified cases in table-driven loops.

## Shared harness

`tests/common/mod.rs` owns security configuration and middleware application,
account/session creation, cookie/CSRF requests, response decoding, row counts,
SQLSTATE assertions and C03 error assertions (including protected fixture sentinels).
Feature test modules retain their probe routes and domain fixtures. Raw security
requests remain available for malformed headers and bodies without Content-Length.
`AGENTS.md` requires future integration tests to extend and reuse this module.

## Duplication gate

`jscpd` is pinned to 4.0.5 in the root manifest and Yarn lockfile. `yarn lint` runs
`yarn lint:duplication` first, so the existing CI lint step enforces it. The config
scans API source/tests and web source, including Rust, TypeScript, TSX, JavaScript,
JSX and CSS. Documentation and fixture data are outside these code formats.

Post-refactor baseline: **121 duplicated lines / 11135 analyzed lines (1.09%)**,
14 clones, with minimum 50 tokens and 5 lines. The threshold is **1.10%**.
The detector's analyzed-line metric differs from physical `wc -l` totals.

Negative check: temporarily copied `services/api/tests/health.rs` to
`services/api/tests/duplication_probe.rs` and ran **`yarn lint`**. It exited **1**
with `jscpd found too many duplicates (1.68%) over threshold (1.1%)`.
Removed the temporary copy and reran `yarn lint`: passed. No ignore directive or
threshold override was needed. The token detector catches copied blocks; the
reuse rule also covers structurally similar helpers that token matching misses.

## Checks

| Check | Result |
| --- | --- |
| yarn lint | Pass: duplication gate, web lint, Rust format and clippy |
| yarn typecheck | Pass |
| yarn test | Pass: 52 Rust tests, real PostgreSQL/SMTP and HTTP fixture servers |
| yarn build | Pass: production web and API builds |
| yarn api:check | Pass: schema validation, 20 contract/client tests and generation drift |
| yarn test:e2e | Pass: 6 real API/PostgreSQL Chromium smoke tests |
| git diff --check | Pass |

No UI changes; new PR screenshots are not applicable. The browser checks cover the
existing foundation, including 390px and 1440px layouts. No live provider credentials
were used. Tests that open local sockets required execution outside the sandbox;
the sandbox-only baseline attempt failed on socket access, not test assertions.
