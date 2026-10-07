# R04 verification — 2026-10-06–07

Prerequisite: issue #1 was closed by merged PR #42 at 08:36:55 UTC; its merge
commit `809c527b07e71fd0a8d212b91fb5c33a30b866f9` is an ancestor of this work.
The working tree was clean before implementation. Changes are confined to R04's
styles, basic primitives, shell, verification files, and the manifest/CI wiring split into R01 PR #58. PR #57 is based on that branch so its review diff contains no R01-owned manifests, lockfile or CI edits. R01 sign-off is pending, not claimed.

## Automated evidence

| Actual command/check | Result |
| --- | --- |
| `yarn install --immutable` | Pass: pinned axe-core dependency and lockfile install reproducibly (existing ESLint peer warning). |
| `yarn test:design` | Pass: generated token/responsive output has no drift; all 8 Node tests pass, including real signal-killed child exit handling. |
| `yarn lint` | Pass: web ESLint, pinned Rust format/clippy. |
| `yarn typecheck` | Pass: Next route generation and TypeScript. |
| `yarn build` | Pass: production web build and pinned Rust build. |
| `yarn api:check` | Pass: OpenAPI validated, 20 contract/client checks, generated client and operation mapping current. No API changes in R04. |
| `yarn test`, with local `.env` loaded by Node and inherited by Yarn | Pass: 12 Rust tests, including real PostgreSQL migrations/constraints/lifecycle and health/SMTP tests. |
| `yarn test:e2e`, with the same local environment | Pass: 6 existing browser tests including same-origin real API/PostgreSQL health checks. This proves the foundation smoke flow, not future account/tracking flows. |
| `yarn test:ui` | Pass: 27 Chromium component/gallery checks using the pinned workspace axe dependency. |
| Gallery production build (`node apps/web/src/components/ui/basic/gallery-server.mjs --build`) | Pass: all gallery fixtures, including SVG hydration and server primitives, compile and render with bundled assets. |

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
and WCAG 2.1 AA, including rendered color contrast. The dependency is pinned in the importing web workspace and lockfile; the suite imports its bundled source without manual downloads or environment variables.

Review refinements added six browser regressions: failed SSR artwork before
JavaScript/hydration, reachable fallback tabs for empty/stale/disabled selection,
busy focus and repeat/form guards, Unicode initials/shared field descriptions,
client navigation preserving the document, and rendered CSS token overrides.
Additional regressions cover valid viewBox-only SVG artwork with simulated zero
intrinsic width, stable ref inspection across renders, flag/family graphemes,
Home's root href/active state, and all seven static primitives rendered on the
server with native choices operating while JavaScript is disabled. The SVG test
allows development StrictMode's initial ref reattachment, then verifies no decode
calls are added by subsequent renders. Eight Node tests cover token normalization,
null omission, diagnostics, changed breakpoints, normal/signal child exits, copied import resolution, and actual SIGKILL-abandoned lease recovery. Ambiguous numeric tokens now fail generation unless explicit typed units are supplied.

A separate server-reuse smoke ran all 19 tests against an already-running preview;
the preview and its pre-existing JavaScript asset still returned 200 afterwards.
The owner then stopped the server. The gallery runner now uses a unique temporary
app outside the production web build directory; no global teardown deletes a
reused server’s files. A full run with graceful owned-server shutdown also checks
that cleanup leaves no temporary app behind.

Immutable install, lint, typecheck, production web/Rust build, real PostgreSQL/SMTP
tests, OpenAPI checks, root browser checks, token drift, and the isolated gallery
production build were rerun after the latest refinements. CI wiring is isolated in R01 PR #58 and invokes
`test:design` and `test:ui` whenever the foundation modules are present; this report records local
execution and does not claim a hosted CI result.

The latest review adds server-page Field render-function coverage; missing
Intl.Segmenter compatibility with complete emoji; parent notification on invalid
Tabs selection and all-disabled focus/content; explicit retryKey recovery of an
unchanged image URL; radio value updates before and after attaching a callback;
and a copied `.ts` helper exercising side-effect, re-export and dynamic imports.
All 27 Chromium tests and 8 Node tests pass. Lint, typecheck, immutable install,
web/Rust build and isolated gallery build were rerun. API/PostgreSQL/root browser
evidence above is retained from the preceding revision, with those modules unchanged.

Two original photographic assets were generated with the built-in image_gen tool
and bundled as WebP (about 380 KB total): a portrait orchard poster and a landscape
harbor still. `ArtworkFigure` composes the existing Poster with semantic captions.
An additional browser check verifies decoded images, local asset responses,
correct 2:3/16:9 crop geometry and retained fallback examples. All 27 browser
checks pass; axe still reports zero violations at mobile/desktop sizes. Lint,
typecheck, design checks and production builds were rerun for this addition.
Prompts and asset provenance are recorded in [artwork/README.md](artwork/README.md).


Photo validation was repeated in a clean detached checkout of the committed PR
with only the photo patch applied, after concurrent unrelated edits appeared in
the shared workspace. The shared checkout reported three regressions from those
Avatar/Tabs edits; they were preserved and excluded from the photo commit. The
isolated result is the evidence for the photo change.

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
routes, migrations, generated-client changes, or shared router edits. R01 PR #58 contains the requested manifest/lockfile and CI updates that register the foundation checks and workspace-scoped dependencies. Merge/review PR #58 before PR #57.
The gallery is a separate verification app under the owned module. Its generated
Next.js output is scoped to a runner-owned temporary app; production routes remain under
R24/R38 integration ownership. See README for preview, checks, cleanup, and props.

R05 (#5) can start once this PR merges. It can compose the provided Button,
fields, focus tokens, and accountAction slot for overlays/menu/feedback. R24/R38
will register the application shell and supply route, beta flag, account, and
notification state. Gallery tests run through `yarn test:ui` and CI. The authored base stylesheet is the single source of truth; only the small responsive media block and handoff tokens are generated, with drift enforced by `test:design`.

Limitations: Chromium verification only; no hardware safe-area or separate
screen-reader session was performed. Overlays/dialog focus, authenticated data,
API-backed feature flows, privacy/spoiler revocation, and other integrated flows
belong to R05 and the feature/integration issues. No new deployment credentials
or services are required by this design foundation.
