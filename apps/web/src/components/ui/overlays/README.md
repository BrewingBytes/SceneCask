# SceneCask overlays (R05)

Import `Dialog`, `ConfirmDialog`, `Sheet`, `Menu`, `ToastViewport`,
`useToastQueue` and their named `*Props`/message types from
`components/ui/overlays`. Every primitive is controlled: the caller owns `open`
(or the toast list) and receives `onOpenChange(false)` / `onDismiss`. Primitives
never fetch API data, run mutations, compute progress, unlock spoilers or infer
permissions. Feature dialogs (catch-up, add sheet, delete account) compose these.

| Component | Public inputs and behavior |
| --- | --- |
| Dialog | `open`, `onOpenChange`, required `title` (accessible name), optional `description` (accessible description), `kicker`, `children`, `footer`, `role` (`dialog`/`alertdialog`), `dismissible` (default true), `busy`, `initialFocusRef`, `returnFocusRef`, `className`. Portaled to `body`; everything else becomes `inert` and the page stops scrolling. Tab/Shift+Tab wrap inside. Escape and a press that starts and ends on the scrim cancel when dismissible and not busy. Initial focus: `initialFocusRef`, then `[data-autofocus]`, then the first tabbable element in the body, then the panel. On close, focus returns to the element focused at open, or `returnFocusRef` when that element is gone (for example a menu item). |
| ConfirmDialog | Dialog props plus `confirmLabel`, `onConfirm`, `cancelLabel` (default Cancel), `destructive`, `confirmDisabled`, `error`. `alertdialog` with Cancel focused first. While `busy`, Confirm blocks repeat activation (R04 Button) and Cancel, Escape and the scrim are blocked, so an in-flight mutation is never silently discarded. `error` renders in an always-mounted alert region and persists until the caller clears it; the caller usually relabels Confirm as Retry. |
| Sheet | Dialog props plus `closeLabel` (default Close). Header close button when dismissible. 560px centered panel from 860px; a bottom sheet with rounded top corners and safe-area padding below. The body scrolls; header and footer stay visible. |
| Menu | `open`, `onOpenChange`, `label` (menu and trigger name), optional `triggerLabel`, `trigger` content, `triggerVariant` (default quiet), `items`, `header`, `align` (`start`/`end`, default end). Items: `id`, `label`, `onSelect`, `href` (Next.js Link), `disabled` (focusable, `aria-disabled`), `tone="destructive"`, `checked` (renders `menuitemcheckbox`). WAI-ARIA menu button: Enter/Space/ArrowDown open on the first item, ArrowUp on the last; arrows wrap; Home/End; type-ahead; Escape and Tab close and focus the trigger; outside presses or focus leaving close. Selecting closes and focuses the trigger before `onSelect` runs, so a dialog opened by an item returns focus to the trigger. Escape in a menu never closes an enclosing dialog. |
| ToastViewport | `toasts`, `onDismiss(id, "timeout" \| "dismiss")`, `label` (default Notifications), `aboveBottomNav` (default true). Mount once near the root. Persistent polite `status` and assertive `alert` regions exist before the first message. Success/info expire after 6s (`TOAST_DURATION_MS`, the handoff token), paused while hovered, focused or busy; errors and `persistent` toasts stay until dismissed. Optional `onUndo`, `onRetry`, one extra `action` (for example Discuss) and an always-present Dismiss. `busy: "undo" \| "retry" \| "action"` shows that action busy, blocks duplicates and Dismiss, and pauses expiry. Portaled outside the page and marked `data-sc-overlay-persist` so an open modal does not make it inert. |
| useToastQueue | `{ toasts, show, update, dismiss }`. `show` returns an id (or reuses `input.id` to replace). Keeps at most `limit` (default 3) success/info toasts, dropping the oldest; errors are never dropped. Holds messages only; callers run Undo/Retry and update or dismiss with the outcome. |

Toast messages and dialog titles/descriptions must be safe product copy: never
provider payloads, exception text, protected episode details or image paths.

Layout follows D01 and the handoff: below 860px dialogs and sheets rise from the
bottom edge and toasts sit 16px above the 72px navigation; from 860px dialogs
(480px) and sheets (560px) are centered and toasts sit 32px from the bottom. The
media query hard-codes 860px because CSS variables cannot appear in media
conditions; a test fails if it drifts from `--sc-layout-breakpoint-wide`. All
controls are at least 44px. Reduced motion removes overlay, menu and toast
animations. Overlays close without an exit animation.

Known limits: a modal makes direct `body` children inert when it opens; nodes
added to `body` while it is open are not made inert. While a modal is open, toast
buttons stay clickable but are outside the Tab cycle. Dismissing a toast does not
move focus anywhere specific.

## Isolated gallery and checks

The verification app under `gallery-app/` renders `OverlayGallery` with
simulated latency and a "Make the next action fail" fixture control. It is not a
production route. It runs on port 3105 through `basic/gallery-runner.mjs`,
the isolated workspace runner now shared with R04's gallery (port 3104).

```sh
node apps/web/src/components/ui/overlays/gallery-server.mjs          # preview
node apps/web/src/components/ui/overlays/gallery-server.mjs --build  # production build check
yarn playwright test --config apps/web/src/components/ui/overlays/playwright.config.ts
```

Root `test:overlays` script and CI registration are R01-owned and proposed in a
separate change. See [verification evidence](verification.md).
