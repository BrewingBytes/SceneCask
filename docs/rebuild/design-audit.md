# Design intake audit — 2026-10-06

Source folder: `/Users/andrei/Downloads/Home directions design exploration`.
Source inspection plus a desktop Home browser spot check. This is not a completed visual/browser QA pass. Home rendered with the featured next-episode band, hidden episode title, out-of-order Paper Moons indicator, and caught-up shows. Mobile and other interactive journeys remain unverified.

## Source hierarchy

- Current handoff: `SceneCask Prototype v2.dc.html`.
- `SceneCask States Gallery.dc.html`: 20 mobile frames at 390px and four desktop frames at 1440px. README incorrectly says 16 mobile; outcomes and gallery source say 20.
- `SceneCask Prototype.dc.html`: historical v1, not the current behavioral contract.
- `SceneCask Home Directions.dc.html`: historical A/B exploration; handoff selects Direction B, Bright editorial.
- `handoff/tokens.json`: explicit color, type, spacing, radius, motion, focus, and responsive tokens.
- `handoff/README.md`, `spec-review.md`, `outcomes.md`: explanatory notes, simulated behavior, and open decisions.
- `support.js`: design runtime only; not application code.

## Current screen and interaction inventory

| Surface | States and actions present in source |
|---|---|
| Welcome/onboarding | Create account, sign in, find show, add/set progress, skip, optional beta follow step |
| Account access | Email/password fields, validation, provider error, busy, verification/resend, reset request/check inbox, sign out |
| Home | Featured next episode, other in-progress shows, single watched action, Undo/Discuss toast, caught-up list, empty library, friend activity in beta |
| Discover/search | Query, loading skeleton, same-title disambiguation by year, results, no results, provider failure/retry, recommendations, add to library |
| Show | Poster/info, season tabs and specials, per-episode watched/unwatched/menu, unknown/future release, next episode, status pills, add/remove, set progress, erase history |
| Episode | Protected/revealed details, still/summary, mark/unmark, explicit catch-up, separate discussion reveal, gap lock, special warning |
| Discussion beta | Hosted thread selection/start, participants, composer/post, author delete, host remove, report/hide, block, reactions |
| Library | Empty, search, status filters/counts, no matches, progress summaries, navigation to show |
| Friends beta | Search, public/private accounts, pending request/withdrawal, follow/unfollow, approve/decline, mutual state, block |
| Shared overlays | Add/set-progress sheet, catch-up counts/ranges/exclusions, reveal confirmation, erase/block/report confirmations, account and row menus |
| Feedback | Success toast with Undo (six seconds), persistent error with Retry/Dismiss, disabled Saving, inline errors, optimistic rollback |

Shared components: navigation shells, typography, buttons, fields, pills, badges, show cards, poster/still placeholders, episode rows, progress bars, season tabs, skeletons, dialogs, sheets, menus, toasts, spoiler notices, avatar/person rows, discussion/comment controls.

## Responsive and accessibility contract candidates

- Breakpoint 860px; mobile gutters 20px, desktop gutters 48px, maximum content width 1240px; episode column 760px.
- Mobile sticky header and fixed 72px bottom navigation; desktop header/pill navigation/search. Alpha has three tabs, beta four despite README's blanket four-tab description.
- Mobile bottom sheets; centered desktop overlays. Lists use auto-fill with a 340px minimum constrained to available width.
- Paper background, ink text, warm accent; Schibsted Grotesk UI, Newsreader reading, IBM Plex Mono episode codes.
- 44px target minimum, 3px focus ring and 2px offset, reduced motion, text/icon state cues, announced feedback. Verify actual components: desktop avatar source is 42px despite stated 44px minimum.
- Posters 2:3, stills 16:9. Final artwork and real navigation icons are absent.

## Gaps and inconsistencies requiring backlog coverage

- Handoff references an unavailable specification. Added rules have since been adjudicated in decisions.md.
- Settings/privacy, profile, follower removal, notifications, export/deletion, operator report review have no completed designs. Password-reset completion and invalid/expired token states need specification.
- Local storage and simulated delays are not authentication, backend persistence, authorization, or integration. Automatic follow approval and the verification button are prototype controls only.
- Source bundles include all sample protected data. Production must omit unauthorized fields from API responses, server rendering, prefetches, caches, and notification payloads; hiding the rendered UI is insufficient.
- Undo compares field values only. A later write that returns to the same value can still be overwritten; use revision/action provenance in the production contract if conflict-safe Undo is approved.
- Discussion reveal state is keyed by episode in the prototype, although copy says this discussion. Production scope must name a discussion identifier if approved.
- Undated episodes can block discussion watched-through while being excluded from bulk catch-up. Decide and explain this explicitly.
- Details reveal survives prototype sign-out in memory; production session grants must have a defined revocation boundary.
- Prototype manual Completed can conflict with derived completion. Resolved by decisions.md.
- Decision update: remove manual Completed; it is computed only. Future episodes must offer individual watched/unwatched actions even though the prototype disables marking them. Keep their future-release labels and exclude them from bulk catch-up/next selection.
- Decision update: email/password plus SSO is required; the supplied account design lacks SSO entry points and linking/error states.
- The gallery's static discussion-lock copy refers to watching one episode, whereas v2 requires watched-through. Canonical copy must follow the approved rule.
- No actual backend failure/permission/concurrency/metadata-import/release-boundary behavior has been verified.
- Local Downloads links are unsuitable as implementation issue references. Publish a versioned design snapshot and stable repository links before issue creation.

## Proposed milestone order (not yet an issue backlog)

1. Record product decisions, durable design snapshot, canonical data/API/design contracts.
2. Repository and CI foundations, database/session infrastructure, shared UI primitives.
3. Account access and metadata search/import.
4. Personal library, per-episode tracking, explicit catch-up, Undo, spoiler-filtered detail views; integrate actual API calls and verify complete solo journeys.
5. Social privacy/following/discovery; integrate permission changes and verify visibility journeys.
6. Hosted episode discussions, moderation and approved notifications; verify spoiler and privacy journeys.
7. Approved account lifecycle features, accessibility/responsive checks, operations and release preparation.

The generated backlog and coverage map supply issue ownership, dependencies, parallel groups and verification. Existing GitHub issues have not been consulted. Missing screens are specified in design/supplement.md; original source remains historical evidence.
