# R04 verification — 2026-10-06

Prerequisite: issue #1 was closed by merged PR #42 at 08:36:55 UTC; its merge
commit `809c527b07e71fd0a8d212b91fb5c33a30b866f9` is an ancestor of this work.
The working tree was clean before implementation. Changes are confined to R04's
styles, basic primitives, shell, and verification files under those modules.

## Automated evidence

| Actual command/check | Result |
| --- | --- |
| `node apps/web/src/styles/generate-tokens.mjs --check` | Pass: verbatim canonical token output has no drift. |
| `yarn lint` | Pass: web ESLint, pinned Rust format/clippy. |
| `yarn typecheck` | Pass: Next route generation and TypeScript. |
| `yarn build` | Pass: production web build and pinned Rust build. |
| `yarn api:check` | Pass: OpenAPI validated, 20 contract/client checks, generated client and operation mapping current. No API changes in R04. |
| `yarn test`, with local `.env` loaded by Node and inherited by Yarn | Pass: 12 Rust tests, including real PostgreSQL migrations/constraints/lifecycle and health/SMTP tests. |
| `yarn test:e2e`, with the same local environment | Pass: 6 existing browser tests including same-origin real API/PostgreSQL health checks. This proves the foundation smoke flow, not future account/tracking flows. |
| `SCENECASK_AXE_SCRIPT=/tmp/scenecask-axe.min.js yarn exec playwright test --config apps/web/src/components/ui/basic/playwright.config.ts` | Pass: 13 Chromium component/gallery checks. |
| Gallery production build (`yarn workspace @scenecask/web exec next build src/components/ui/basic/gallery-app`) | Pass: gallery/alpha/art fixtures compile and render with bundled assets. |

The gallery suite measured no horizontal document overflow at 320, 390, 859,
860, and 1440px. Navigation is fixed below 860 and in the header at 860 and
above. All visible shell/button/field/choice-label targets measured at least
44×44px; the 20px checkbox/radio itself has a full clickable 44px label.

Keyboard checks covered skip-link → main focus, named field hints/errors,
disabled/busy buttons, native radio navigation, tab arrow wrap and End while
skipping disabled options, associated visible panels, persistent Retry and
explicit Dismiss. Focus was measured at 3px with a 2px offset at 390/1440.

Artwork checks covered absent sources, failed images, source-change recovery,
constant geometry before/after successful and failed loads, and both 2:3/16:9
ratios. Reduced-motion checks confirmed animation/transition removal. Browser
network inspection found no external gallery requests; all three font families
loaded locally.

Axe-core 4.10.3 reported **zero violations** at 390 and 1440px for WCAG 2 A/AA
and WCAG 2.1 AA, including rendered color contrast. The dependency was fetched
into a temporary file; no shared package manifest/lockfile changed.

After improving the error-state screenshot framing and formatting the source,
the two 390/1440 responsive checks were rerun successfully; lint/typecheck and
token drift checks were also rerun. Screenshots below are the final captures.

## Manual visual review

Inspected the full 390/1440 gallery screenshots and the corresponding empty/error
screenshots. Checked warm-paper/ink/rust styling, bundled typography, mobile
wrapping, visible navigation labels, striped art geometry, readable inline
errors, and recovery actions. Also inspected browser screenshots of keyboard
focus on the selected tab at both widths: the complete accent outline is visible
and not clipped. Keyboard interactions and reduced motion are automated evidence,
not a claim of a separate human-operated keyboard session.

- [390px gallery](verification/gallery-390.png)
- [1440px gallery](verification/gallery-1440.png)
- [390px empty/error states](verification/states-390.png)
- [1440px empty/error states](verification/states-1440.png)

## Scope and handoff

No contract deviations, prototype controls/support.js imports, API calls, domain
routes, migrations, manifests, generated-client changes, or shared router edits.
The gallery is a separate verification app under the owned module. Its generated
Next.js output is removed by the test teardown; production routes remain under
R24/R38 integration ownership. See README for preview, checks, cleanup, and props.

R05 (#5) can start once this PR merges. It can compose the provided Button,
fields, focus tokens, and accountAction slot for overlays/menu/feedback. R24/R38
will register the application shell and supply route, beta flag, account, and
notification state. Gallery tests are run by the explicit module command; CI
registration remains coordinated with R01/integration ownership.

Limitations: Chromium verification only; no hardware safe-area or separate
screen-reader session was performed. Overlays/dialog focus, authenticated data,
API-backed feature flows, privacy/spoiler revocation, and other integrated flows
belong to R05 and the feature/integration issues. No new deployment credentials
or services are required by this design foundation.
