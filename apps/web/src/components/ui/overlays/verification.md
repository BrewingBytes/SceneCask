# R05 verification — 2026-10-07

Prerequisite: R04 (#4) is closed by merged PR #57 (`3601f32`), and R01's
foundation wiring PR #58 (`c97049d`) is merged. Both are ancestors of this work.
The working tree was clean before implementation. Changes stay inside
`apps/web/src/components/ui/overlays/**`, except that R04's gallery launcher now calls
a shared `basic/gallery-runner.mjs` (R04/R05 shared-primitive ownership; behavior unchanged). Registering the overlay suite in root
scripts and CI is R01-owned and proposed separately. Until that lands, CI does
not run this suite.

## Automated evidence

| Actual command/check | Result |
| --- | --- |
| `yarn playwright test --config apps/web/src/components/ui/overlays/playwright.config.ts` (CI=1, owned server) | Pass: 29 Chromium tests. |
| Same, `--repeat-each=5` | Pass: 145/145. Toast tests also 30/30 at `--repeat-each=10` after pausing the fake clock (an earlier run flaked when real latency advanced running fake time). |
| Regression tests vs. reverted fixes | Each of menu Tab propagation, same-commit nesting, late-portal inert, re-issued toast id and type-ahead prefix fails with its fix reverted and passes with it. Second review round: footer/header nesting depth, recording the trigger once (StrictMode remount), deferred focus restore after same-commit parent+child close, window-level Escape/Tab with focus on body, and same-copy toast re-show likewise fail when reverted. Third review round: open-order stacking of an unrelated page dialog above a nested layer, ignoring `body` as the recorded trigger (so `returnFocusRef` applies), IME-composition Escape, Escape from the toast region, toasts moving above a bottom-anchored modal, and busy toast actions disabling sibling actions likewise fail when reverted. Outermost-first restore ordering is not independently distinguishable (a closed child's trigger is always inside its closed parent). The stale-focus toast timer, `summary`/media tabbables and the persist-constant refactor, disabled-fieldset/`visibility:hidden` tabbable filtering and the menu entry-point reset have no dedicated test. |
| `node apps/web/src/components/ui/overlays/gallery-server.mjs --build` | Pass: the fixture app compiles and prerenders. |
| `yarn lint` | Pass: web ESLint (react-hooks compiler rules), Rust fmt/clippy. |
| `yarn typecheck` | Pass. |
| `yarn build` | Pass: production web build and Rust build. |
| `yarn test:design` | Pass: 9 Node tests, no token drift. |
| `yarn test:ui` (R04 suite) and R04 gallery `--build` | Pass: 27 tests and build through the shared runner. |
| `yarn api:check` | Pass: client current. No API changes in R05. |
| `yarn test` with local `.env` (PostgreSQL 18 container) | Pass: 12 Rust tests. No backend changes. |
| `yarn test:e2e` with local `.env` | Pass: 6 tests. No integration changes; this is not overlay integration evidence. |

What the overlay suite proves:

- **Keyboard-only dialog (acceptance 1).** Tab reaches the trigger and Enter opens
  the dialog. Cancel has initial focus. Three Tab and three Shift+Tab presses keep
  focus inside. Every other body child is `inert`. Escape, and separately Enter on
  Cancel, close the dialog and return focus to the trigger. Afterwards no `inert`
  remains and no action started.
- **Sheet.** Escape, a scrim press and the Close button each return focus to the
  trigger. Body scrolling is locked while open and restored after.
- **Nesting.** A confirmation inside a sheet traps focus. Escape closes only the
  confirmation, then the sheet, and focus returns each time. Layers opened in the
  same commit (confirmation in the sheet footer) stack by nesting depth; closing
  the confirmation moves focus into the sheet, and closing the sheet returns it to
  the page trigger. Closing sheet and confirmation together (Remove success)
  returns focus to the page trigger with no `inert` left. Escape and Tab work after
  focus falls to `body`. A portal appended while a modal is open becomes inert.
- **Busy and failure (acceptance 2).** While busy, repeated click and Enter, Escape,
  Cancel and the scrim leave the dialog open, and exactly one action starts. The
  failure alert persists after 1s, Retry succeeds, focus returns, and a success
  status is announced.
- **Toasts (acceptance 2, fake clock).** Both live regions exist before the first
  message. Success is present at 5.9s and gone at 6.1s. The error is still present
  after a further 60s, with Retry and Dismiss. Busy Retry runs once, then the error
  is replaced by a success. Dismiss removes the error. Focus pauses expiry, which
  resumes after blur. Re-showing a toast id, with new or identical copy, restarts the 6s.
- **Menu.** `aria-haspopup`/`aria-expanded` are set. Enter, Space, ArrowDown and
  ArrowUp open the menu. Arrows wrap; Home, End and type-ahead (including a
  multi-character prefix) work. A disabled item stays focusable but cannot be
  selected. `menuitemcheckbox` toggles. Escape and Tab return focus to the trigger;
  outside presses close the menu. A dialog opened from a menu item returns focus to
  the menu trigger. Inside a sheet, Escape and Tab in the menu keep the sheet open
  and focus on the trigger, and the footer menu opens upward, unclipped.
- **Reduced motion.** With reduced motion, the scrim, panel, toast and menu
  animations compute to `none`. Without it, the panel animates.
- **Responsive.** At 320, 390, 859, 860 and 1440px there is no horizontal document
  overflow, and every overlay surface sits inside the viewport. Dialogs and sheets
  are bottom-anchored below 860px and centered from 860px, at 480px and 560px
  wide. Toasts clear the 72px navigation by 16px on mobile and sit 32px from the
  bottom on desktop. All overlay controls measure at least 44×44px.
- **Breakpoint.** A test fails if the 860px media query in `overlays.css` drifts
  from `--sc-layout-breakpoint-wide`.
- **Accessibility.** axe-core 4.10.3 (WCAG 2 A/AA, 2.1 AA, including contrast)
  reports zero violations at 390 and 1440px with the dialog, sheet, menu, both
  toasts and the dialog failure state open.

## Screenshots (generated by the suite)

| State | 320 | 390 | 1440 |
| --- | --- | --- | --- |
| Confirm dialog | [320](verification/dialog-320.png) | [390](verification/dialog-390.png) | [1440](verification/dialog-1440.png) |
| Dialog failure + Retry | — | [390](verification/dialog-error-390.png) | [1440](verification/dialog-error-1440.png) |
| Sheet | [320](verification/sheet-320.png) | [390](verification/sheet-390.png) | [1440](verification/sheet-1440.png) |
| Account menu | [320](verification/menu-320.png) | [390](verification/menu-390.png) | [1440](verification/menu-1440.png) |
| Success + error toasts | [320](verification/toasts-320.png) | [390](verification/toasts-390.png) | [1440](verification/toasts-1440.png) |

## Manual evidence

I inspected the screenshots against prototype v2:

- Overlays use a paper surface on the 0.45 scrim, with 20px top radii on mobile
  and 20px all round when centered.
- Toasts are ink for success and error red for failures, with paper pill actions.
- The account menu is a raised popover with a hairline border.

No screen reader was used. The live-region and accessible-name behavior is
verified only through the accessibility tree and axe.

## Limitations and follow-ups

- **CI:** the suite is not run by CI until R01 registers a `test:overlays` script
  and CI step (separate change).
- **Not integration evidence:** the gallery fixtures simulate latency and failure.
  Real mutations, Undo semantics and copy belong to feature issues and to the
  R24/R38 integration gates.
- **Inert scope:** only direct `body` children are made inert, including ones added
  while a modal is open. Toasts stay interactive during a modal, but they are
  outside its Tab cycle.
- **Dismissed toasts:** dismissing a toast leaves focus on `body`.
- **Out of scope:** SpoilerNotice (listed with R04/R05 primitives in D01) is not
  part of issue #5's requirements and is not implemented here.
- **Unblocked:** features that need dialogs, sheets, menus or toasts can start
  composing these, for example catch-up confirm, the add sheet and the account menu.
