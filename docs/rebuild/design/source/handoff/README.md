# SceneCask: design handoff (Direction B, Bright editorial)

## Files
- `SceneCask Prototype v2.dc.html`: the current prototype, aligned to the product spec. It is one responsive file that reflows at 860px. **See `spec-review.md` for what changed.**
- `SceneCask Prototype.dc.html`: v1, kept for comparison.
- `SceneCask States Gallery.dc.html`: 16 mobile and 4 desktop frames of the live prototype in fixed states, plus a component and token sheet.
- `SceneCask Home Directions.dc.html`: the original A/B exploration, kept for reference.
- `handoff/tokens.json`: color, type, spacing, radius, motion and layout tokens.
- `support.js`: the runtime that opens the `.dc.html` files. It is not product code.

### Deep links (for QA)
`SceneCask Prototype v2.dc.html?start=new|returning|caught|empty&beta=1&auth=create|signin|verify&screen=home|discover|library|friends|show|episode&id=ho&s=2&e=8&reveal=details|disc&q=hollow&sheet=1&dialog=through&fail=1&meta=1`

Deep-linked frames don't read or write saved progress. Only the plain URL persists.

## Assets
There are no final assets. Posters and episode stills are striped placeholders labeled "POSTER ART" and "EPISODE STILL". They are filled with a per-show color pair.
- Posters are 2:3 and stills are 16:9. Provide 2x images, and use the show title as the alt text.
- The Home "next up" band and progress bars use a poster-derived color. Pick the foreground color to reach at least 4.5:1, falling back to ink or paper.
- Icons in the bottom nav are geometric placeholders. Replace them with a 24px outline icon set, and keep the visible labels.
- Fonts (Google Fonts): Schibsted Grotesk, Newsreader, IBM Plex Mono.

## Layout and responsive behavior
- **Below 860px:** a sticky top bar with the wordmark and account button, plus a fixed bottom nav (72px, four labeled tabs). Gutters are 20px. Sheets and dialogs slide up from the bottom.
- **860px and above:** a compact top bar with the wordmark, pill nav, search field and account button. Content is capped at 1240px, with 48px gutters. Sheets and dialogs are centered.
- Lists use `grid auto-fill minmax(min(340px,100%),1fr)`, so they become one column on mobile and two on desktop. At 320px everything stays a single column, and controls wrap.
- **Home:** Up next (featured band, then other shows in progress), then Caught up, then From friends. Personal tracking always comes before social.
- **Show detail:** poster and info wrap, then season tabs (Specials is its own tab), then episode rows.
- **Episode:** 760px column. Header, spoiler notice, content (or a note that it's hidden), watched actions, then Discussion.

## Rules
The product spec is now the source of truth. `spec-review.md` maps each rule to how the prototype implements it, and marks which rules are confirmed (C) and which are recommended defaults (R).

## Interactions
| Action | Feedback | Failure |
|---|---|---|
| Mark watched (Home, Show, Episode) | Optimistic update; toast "Marked S2 E7 watched" with Undo and Discuss (6s) | After about 0.9s it reverts, and an error toast offers Retry and Dismiss (it stays until acted on) |
| Mark unwatched | Toast with Undo | Revert and Retry |
| Mark watched through… | Confirm dialog with exact count and ranges, then toast with Undo | Revert and Retry |
| Add to library / Set progress | Sheet: season tabs, episode grid, live scope summary; Save shows "Saving…" (disabled) | Inline error, nothing changed, button becomes Retry |
| Add a recommendation | Added as Plan to watch; button shows "✓ Plan to watch"; toast with Undo | Revert and Retry |
| Change status | Radio pills; toast with Undo | Revert and Retry |
| Search | 550ms skeleton, then results; no results and error states | "Search isn't responding" with Try again |
| Follow request | "Requested", then a simulated approval after 2.5s with a toast | (Rui never approves, so the pending state stays) |
| Approve / Decline request | Toast explains what the person can now see | |
| Report comment | Reason radio group; comment hidden for you; toast | |
| Block | Confirm dialog explaining scope; toast with Undo | |

## Accessibility
- Touch targets are at least 44px, and the focus ring is 3px accent with a 2px offset. Esc closes sheets, dialogs and menus.
- Status is never shown by color alone: watched states use ✓ and text, upcoming rings are dashed, and badges are text.
- Text meets 4.5:1 on paper (ink-muted is about 6:1). Text on poster colors must be checked for each show.
- Roles used: progressbar (with value), radiogroup (status, report reasons), tablist (seasons), alertdialog (confirms), status and alert live regions (toasts, errors), menu (account, row actions), aria-current (nav), aria-pressed (toggles, reactions).
- Under prefers-reduced-motion, all transitions are disabled.

## What is simulated
- **There is no backend.** Progress is saved to this browser's storage so it survives reloads, standing in for the account. Saves are optimistic, with timeouts. Reveals are kept in memory only, so they last for the session.
- **Account access:** any valid email works. "I've verified my email" stands in for the link. Signing in with an empty library loads Ana's sample data.
- **Alpha/Beta switch** in the account menu. Alpha is the first-release scope; Beta previews the social features.
- **Metadata update** control: releases Night Shift S4 E12.
- **Failures** come from the account menu, under Prototype controls: "Make the next save fail" (or `&fail=1`). It affects the next save, sheet save or search.
- **Follow approval** is automatic after 2.5s for the sample people. Rui stays pending.
- **Preset users** (new, returning Ana, caught up, empty library) can be loaded from the account menu.
- **Not built:** Profile, Settings & privacy (public-account switch, remove followers), notifications, export and deletion. See `spec-review.md`.
- Reaction counts, comment timestamps and episode titles are generated sample data. None of it contains plot spoilers.

## Sample data (consistent everywhere)
Ana Reyes, @ana.r. Returning preset:

- Watching: Hollow Orchard through S2 E6 (next S2 E7); The Lantern Coast through S1 E3; Paper Moons through S1 E2 (E5 airs Tue, Oct 6); Night Shift at Meridian through S4 E11 (caught up, E12 airs Fri, Oct 9); Saltwater Kings, all of S3 (caught up, S4 date not announced).
- Other statuses: The Quiet Year (Completed); Glasshouse (Plan to watch); Tidewater Radio (On hold).
- People: Ana follows Maya, Priya and Jonas. Theo has requested to follow Ana.
