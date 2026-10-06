# SceneCask design foundation (R04)

Import primitives and their named `*Props` types from `components/ui/basic`.
Import `AppShell` and `AppShellProps` from `components/shell`. Imports load
`styles/foundation.css`, the generated handoff tokens, and licensed local fonts.
Render primitives inside `AppShell` or a `.sc-foundation` wrapper for typography.

The shell takes `currentPath`, `betaEnabled` (default false), `unreadCount`, and
an optional `accountAction` slot. The application integration owner supplies
router state, the authoritative beta flag, and account/notification data. Alpha
shows Home/Discover/Library; beta adds Friends and notification access in the
header. Navigation uses real canonical hrefs, with no feature route registration
in this change. The account slot is where R05's accessible menu can be composed.
The shell includes a skip link and focusable main landmark. Mobile navigation is
72px plus the safe-area inset; desktop starts at exactly 860px.

| Component | Public inputs and behavior |
| --- | --- |
| Button | Native button props; primary/secondary/quiet/destructive, disabled, busy. Default type is button. Busy keeps its accessible name and disables repeated activation. |
| TextField / TextArea | Native input props plus required label, optional hint/error. Stable generated IDs join labels and descriptions. Error sets aria-invalid. |
| Checkbox | Native checkbox props and a required label; the whole label is a 44px target. |
| RadioGroup | Controlled value/onChange, group label/name, labeled options, optional disabled states. Native arrow-key behavior. |
| Badge | Children and neutral/accent/error tone; informational, not interactive. |
| Tabs | Controlled value/onChange, group label, unique value/label/content options. Supply an enabled selected value. Roving focus, arrow wrap, Home/End, disabled option skipping, and linked panels. |
| Progress | Required label/value; optional max (default 100). Bounds invalid/nonfinite values without displaying false progress. |
| Poster | Authorized src or null, required safe alt (empty for decorative art), poster/still aspect, optional className. Reserved 2:3/16:9 geometry, striped fallback on missing/failed images, recovery when src changes. |
| Avatar | Initials and accessible label; fixed 44px, no uploads. |
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

The canonical handoff JSON is read directly without changing its values. CSS
custom properties keep the source numbers as unitless values; multiply by 1px
when consuming dimensional numeric tokens. Foundation CSS applies D01's
functional layout, contrast, focus, and reduced-motion rules.

```sh
node apps/web/src/styles/generate-tokens.mjs
node apps/web/src/styles/generate-tokens.mjs --check
```

## Isolated gallery and checks

`ComponentGallery` exports isolated examples. Its separate Next.js verification
app lives under this module, with beta `/`, alpha `/alpha`, and an artwork test
fixture `/art`. None of these routes belongs to the production application.

From the repository root:

```sh
yarn workspace @scenecask/web exec next dev src/components/ui/basic/gallery-app --hostname 127.0.0.1 --port 3104
```

For the automated Chromium checks, obtain the pinned axe script without changing
R01's manifests, then run:

```sh
curl -fsSL https://unpkg.com/axe-core@4.10.3/axe.min.js -o /tmp/scenecask-axe.min.js
SCENECASK_AXE_SCRIPT=/tmp/scenecask-axe.min.js yarn exec playwright test --config apps/web/src/components/ui/basic/playwright.config.ts
```

The suite starts its own gallery server and removes only its generated Next.js
artifacts on completion. If a manually started server was reused, stop it before
running repository lint/typecheck. A standalone production-build check is:

```sh
yarn workspace @scenecask/web exec next build src/components/ui/basic/gallery-app
node -e 'import("./apps/web/src/components/ui/basic/gallery-teardown.mjs").then(m=>m.default())'
```

The cleanup also applies after manually previewing the gallery, so nested build
output never enters the production lint/typecheck inputs. Root manifests, CI,
application routers, and the real web/API browser configuration retain their
separate ownership. The gallery suite is an additional explicit command.

See [verification evidence](verification.md) for actual results and screenshots.
