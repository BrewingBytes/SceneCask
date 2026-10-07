# SceneCask design foundation (R04)

Import primitives and their named `*Props` types from `components/ui/basic`.
Import `AppShell` and `AppShellProps` from `components/shell`. Imports load
`styles/foundation.css`, the generated responsive rules and handoff tokens, and licensed local fonts.
Render primitives inside `AppShell` or a `.sc-foundation` wrapper for typography.
Each primitive has its own file (for example `button.tsx`, `poster.tsx`, and
`tabs.tsx`); barrel exports preserve existing imports and named props. `Field`
is also exported for custom controls sharing label/hint/error wiring. It has no client boundary, so server pages can pass its render function directly. The shell
exports reusable `SiteHeader`, `Navigation`, `BrandLink`, `NotificationLink`,
`PageContainer`, and `NavIcon` components.

The shell takes `currentPath`, `betaEnabled` (default false), `unreadCount`, and
an optional `accountAction` slot. The application integration owner supplies
router state, the authoritative beta flag, and account/notification data. Alpha
shows Home/Discover/Library; beta adds Friends and notification access in the
header. Navigation uses Next.js Link for client transitions and automatic prefetching with no feature route registration
in this change. Home and the brand default to `/`, the existing production landing route. Integration can pass `homeHref="/home"` once that route is registered. The account slot is where R05's accessible menu can be composed.
The shell includes a skip link and focusable main landmark. Mobile navigation is
72px plus the safe-area inset; desktop starts at exactly 860px.

| Component | Public inputs and behavior |
| --- | --- |
| Button | Native button props; primary/secondary/quiet/destructive, disabled, busy. Default type is button. Busy keeps its accessible name and focus, sets aria-disabled/aria-busy, and blocks repeated activation and form submission without hiding the click from ancestor listeners. Explicit disabled still uses the native attribute. |
| Field | Reusable render-prop wrapper with label, hint, error, id and describedBy; passes a generated control ID and accessibility props to custom controls. Errors render in a polite live region so they are announced when they appear without interrupting; use ErrorState for an assertive form-level alert. |
| TextField / TextArea | Native input props plus required label, optional hint/error. Stable generated IDs join labels and descriptions. Error sets aria-invalid. |
| Checkbox | Native checkbox props and a required label; the whole label is a 44px target. |
| RadioGroup | Controlled `value` (with or without a callback), or uncontrolled `defaultValue` for native server rendering; group label/name, labeled options, optional disabled states. Native arrow-key behavior. |
| Badge | Children and neutral/accent/error tone; informational, not interactive. |
| Tabs | Controlled value/onChange, group label, unique value/label/content options. Empty/stale/disabled selections render the first enabled tab/panel without calling onChange, so a deep-linked value survives until its tab becomes available. When all tabs are disabled, the tablist remains focusable, an unavailable status appears, and the panel matching the controlled value (or the first panel) stays visible. Hidden panels stay mounted so their state survives tab switches. Roving focus, arrow wrap, Home/End, disabled option skipping, and linked panels. |
| Progress | Required label/value; optional max (default 100). Bounds invalid/nonfinite values without displaying false progress. |
| Poster | Authorized src or null, required safe alt (empty for decorative art), poster/still aspect, optional className. Reserved 2:3/16:9 geometry, striped fallback on missing/failed images including failures before hydration, decode validation for zero intrinsic width SVGs, recovery when src changes or the caller changes retryKey to retry an unchanged URL. |
| Avatar | Initials and accessible label; Lazy Intl.Segmenter with a pinned Graphemer fallback when unavailable, preserving full graphemes, fixed 44px, no uploads. |
| Skeleton | Optional width/height. Requires either `label` (one Skeleton per loading area, announced as a status) or `decorative` (hidden from assistive technology); reduced motion disables pulse. |
| EmptyState | Title, optional children/action. |
| ErrorState | Safe title/children, optional onRetry/onDismiss and retry busy state. Alert announces the failure; errors persist until explicitly dismissed or replaced by the caller. |

Use `.sc-reading-column` (760px), `.sc-form-column` (480px), `.sc-reading`,
`.sc-code`, `.sc-kicker`, and `.sc-actions` to compose feature layouts. Reading text
uses Newsreader; UI/headings use Schibsted Grotesk; codes use IBM Plex Mono.
All font requests are same-origin assets. SIL OFL licenses accompany the fonts.
Lucide paths used by the shell retain their license in `shell/LICENSE-lucide`.

`Poster` does not authorize content. Callers must supply backend-filtered artwork
and safe labels; never pass hidden episode strings or paths. Error copy should be
product text, never an exception/provider payload. Primitives do not fetch API
data, unlock spoilers, mutate progress, or infer permissions.

## Token regeneration

The canonical handoff JSON stays unchanged. The generator converts dimensions to
px, resolves border/focus color and font references, extracts toast durations,
and omits null/instructional tokens. Foundation CSS uses those tokens for type,
spacing, radii, targets, gutters and motion. Edit the authored `foundation.css`
directly. Only `foundation-responsive.template.css` generates a CSS copy: its
media-query breakpoint comes from the handoff because CSS variables cannot
appear in media conditions. `styles/index.ts` loads responsive rules after the
base stylesheet. D01's reading and form widths remain centralized supplement tokens.

```sh
node apps/web/src/styles/generate-tokens.mjs
yarn test:design
```

`test:design` checks generated token/responsive output for drift and runs the
Node generator, child-process exit and gallery-workspace tests. CI runs it on every pull request.

New numeric tokens outside the legacy handoff schema must carry explicit type
metadata. Unsupported or ambiguous numbers fail generation, including `--check`.
For example:

```json
{"focus":{"gap":{"$type":"dimension","$value":{"value":0.5,"unit":"rem"}}},"motion":{"delay":{"$type":"duration","$value":{"value":120,"unit":"ms"}}}}
```

Supported numeric types are dimension, duration and number. Approved handoff
values remain unchanged. Unitless weight/line-height values are validated separately.

## Isolated gallery and checks

`ComponentGallery` exports isolated examples. Its separate Next.js verification
app lives under this module, with beta `/`, alpha `/alpha`, and an artwork test
fixture `/art`. None of these routes belongs to the production application.

From the repository root:

```sh
node apps/web/src/components/ui/basic/gallery-server.mjs
```

For the automated Chromium checks, install workspace dependencies and Chromium,
then run:

```sh
yarn install --immutable
yarn playwright install chromium
yarn test:ui
```

The suite imports axe-core 4.10.3 from the pinned workspace dependency; no manual
download or environment variable is needed. CI runs it after the real API browser
suite and uploads the browser evidence.

The runner parses module specifiers with TypeScript and copies the verification app into a unique repository-root `.next`
directory, separate from the production web build. It deletes only that directory
when its child exits. Playwright requests graceful shutdown for servers it starts;
when it reuses a developer’s server, it does not launch the runner or delete any
server artifacts. Static, side-effect, re-export and literal dynamic imports are handled in TS/JS files; internal imports point to copied files and external relative imports point to their original source. On the next startup, recorded abandoned directories are reaped only when both owner and child processes have exited. SIGKILL cannot run immediate cleanup. No global teardown runs. Preview servers remain live until
stopped by their owner.

A standalone production-build check uses the same isolated runner and cleanup:

```sh
node apps/web/src/components/ui/basic/gallery-server.mjs --build
```

Generated output stays outside source inputs, so repository lint/typecheck/build
can run while a gallery preview is open. The R01-owned manifest/CI wiring in PR #58 adds `test:design` and `test:ui` without changing
application routers or the real web/API browser configuration. Static primitives
can be imported directly by server components; native checkbox/radio interaction
is also verified with JavaScript disabled.

See [verification evidence](verification.md) for actual results and screenshots.
