# SceneCask design foundation (R04)

Import primitives and their named `*Props` types from `components/ui/basic`.
Import `AppShell` and `AppShellProps` from `components/shell`. Imports load
`styles/foundation.css`, the generated handoff tokens, and licensed local fonts.
Render primitives inside `AppShell` or a `.sc-foundation` wrapper for typography.
Each primitive has its own file (for example `button.tsx`, `poster.tsx`, and
`tabs.tsx`); barrel exports preserve existing imports and named props. `Field`
is also exported for custom controls sharing label/hint/error wiring. The shell
exports reusable `SiteHeader`, `Navigation`, `BrandLink`, `NotificationLink`,
`PageContainer`, and `NavIcon` components.

The shell takes `currentPath`, `betaEnabled` (default false), `unreadCount`, and
an optional `accountAction` slot. The application integration owner supplies
router state, the authoritative beta flag, and account/notification data. Alpha
shows Home/Discover/Library; beta adds Friends and notification access in the
header. Navigation uses Next.js Link for client transitions and automatic prefetching with real canonical hrefs, with no feature route registration
in this change. The account slot is where R05's accessible menu can be composed.
The shell includes a skip link and focusable main landmark. Mobile navigation is
72px plus the safe-area inset; desktop starts at exactly 860px.

| Component | Public inputs and behavior |
| --- | --- |
| Button | Native button props; primary/secondary/quiet/destructive, disabled, busy. Default type is button. Busy keeps its accessible name and focus, sets aria-disabled/aria-busy, and blocks repeated activation and form submission. Explicit disabled still uses the native attribute. |
| Field | Reusable render-prop wrapper with label, hint, error, id and describedBy; passes a generated control ID and accessibility props to custom controls. |
| TextField / TextArea | Native input props plus required label, optional hint/error. Stable generated IDs join labels and descriptions. Error sets aria-invalid. |
| Checkbox | Native checkbox props and a required label; the whole label is a 44px target. |
| RadioGroup | Controlled value/onChange, group label/name, labeled options, optional disabled states. Native arrow-key behavior. |
| Badge | Children and neutral/accent/error tone; informational, not interactive. |
| Tabs | Controlled value/onChange, group label, unique value/label/content options. Empty/stale/disabled selections render the first enabled tab/panel without changing the caller’s value. Roving focus, arrow wrap, Home/End, disabled option skipping, and linked panels. |
| Progress | Required label/value; optional max (default 100). Bounds invalid/nonfinite values without displaying false progress. |
| Poster | Authorized src or null, required safe alt (empty for decorative art), poster/still aspect, optional className. Reserved 2:3/16:9 geometry, striped fallback on missing/failed images including failures before hydration, recovery when src changes. |
| Avatar | Initials and accessible label; Unicode code-point-safe truncation, fixed 44px, no uploads. |
| Skeleton | Accessible loading label and optional width/height; reduced motion disables pulse. |
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
spacing, radii, targets, gutters and motion. Edit `foundation.template.css`, then
regenerate `foundation.css`. Its media-query breakpoint is generated from the
handoff because CSS variables cannot appear in media conditions. D01's 760px
reading and 480px form widths are centralized supplement tokens in the template.

```sh
node apps/web/src/styles/generate-tokens.mjs
node apps/web/src/styles/generate-tokens.mjs --check
node --test apps/web/src/styles/generate-tokens.test.mjs
```

## Isolated gallery and checks

`ComponentGallery` exports isolated examples. Its separate Next.js verification
app lives under this module, with beta `/`, alpha `/alpha`, and an artwork test
fixture `/art`. None of these routes belongs to the production application.

From the repository root:

```sh
node apps/web/src/components/ui/basic/gallery-server.mjs
```

For the automated Chromium checks, obtain the pinned axe script without changing
R01's manifests, then run:

```sh
curl -fsSL https://unpkg.com/axe-core@4.10.3/axe.min.js -o /tmp/scenecask-axe.min.js
SCENECASK_AXE_SCRIPT=/tmp/scenecask-axe.min.js yarn exec playwright test --config apps/web/src/components/ui/basic/playwright.config.ts
```

The runner copies the verification app into a unique repository-root `.next`
directory, separate from the production web build. It deletes only that directory
when its child exits. Playwright requests graceful shutdown for servers it starts;
when it reuses a developer’s server, it does not launch the runner or delete any
server artifacts. No global teardown runs. Preview servers remain live until
stopped by their owner.

A standalone production-build check uses the same isolated runner and cleanup:

```sh
node apps/web/src/components/ui/basic/gallery-server.mjs --build
```

Generated output stays outside source inputs, so repository lint/typecheck/build
can run while a gallery preview is open. Root manifests, CI, application routers,
and the real web/API browser configuration retain their separate ownership.
The gallery suite is an additional explicit command.

See [verification evidence](verification.md) for actual results and screenshots.
