# SceneCask: outcomes (Oct 5, 2026)

## Deliverables
- **Prototype:** `SceneCask Prototype v2.dc.html`. It's aligned to the product spec and opens in Alpha (personal tracking) by default; Beta (social) is behind a switch in the account menu.
- **States gallery:** `SceneCask States Gallery.dc.html`. It has 20 mobile (390) and 4 desktop (1440) live frames, plus the component and token sheet.
- **Design tokens:** `handoff/tokens.json` (color, type, spacing, radius, motion, layout, focus).
- **Developer handoff:** `handoff/README.md` covers layouts, responsive behavior, interactions, accessibility, assets and what's simulated.
- **Spec review:** `handoff/spec-review.md` lists the conflicts found, the fixes and open questions. Each rule is marked confirmed (C) or recommended (R).
- **History:** `SceneCask Home Directions.dc.html` (the A/B exploration) and `SceneCask Prototype.dc.html` (v1).

## Visual check (desktop, about 920px)
Passed, with no console errors:
- Home (returning): the next episode band and Paper Moons out of order (next is E2).
- Home (caught up): "Waiting on new episodes" shows dates or "date not announced".
- Show (Saltwater Kings): canceled show, S2 E5 undated, shows Caught up rather than Completed. The header fits.
- Catch-up dialog: the undated episode and already-watched episodes are listed as not included.
- Episode, alpha: details are absent, with a separate "Reveal episode details…" action and the bulk action.
- Episode, beta: discussion reveal notice; the Friends tab appears only in beta.
- Friends (beta): private-account copy, incoming request, approve and decline.
- Create account: labels and password hint.

Mobile (390) layouts are covered by the gallery frames and were not captured in this pass.

## Open decisions for product
1. Approve or adjust the recommended (R) rules in `spec-review.md`.
2. Library statuses (Plan to watch … Completed) aren't in the spec. Keep them, or replace them with derived states only?
3. Episode reactions (beta) aren't in the spec. Keep them or cut them?
4. Next pass: Settings and privacy (public-account switch, remove followers), notifications, data export, account deletion.
