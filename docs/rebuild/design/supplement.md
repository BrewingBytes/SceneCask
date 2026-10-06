# Design contract and missing-screen supplement

This supplement is authorized by the user's instruction to specify missing screens using the existing design system. Canonical behavior: ../contracts.md. Original export remains unmodified under source/. All issue references use versioned GitHub paths; run `python3 -m http.server 8765 --directory docs/rebuild/design/source` locally to view exports. support.js is only a design runtime, never a product dependency.

## D01 — Shared visual contract

Use source/handoff/tokens.json verbatim except functional corrections here. Bright editorial: warm paper, dark ink, rust accent; Schibsted Grotesk headings/UI, Newsreader reading paragraphs, IBM Plex Mono episode codes/kickers. Bundle fonts via build-time download or checked licensed assets; auth-token pages must not load external font URLs. Use a consistent 24px outline icon set (Lucide), with visible nav labels. Initials avatars only.

Breakpoint 860px. At 320/390/768 use 20px gutters, sticky header, bottom nav72px and safe-area inset; desktop 860/1024/1440 uses48px gutters capped1240px. Alpha navigation Home/Discover/Library; beta adds Friends and notification access. Episode reading column max760px. Forms max480px. Header and controls wrap without horizontal page scroll. Poster2:3, still16:9 with explicit dimensions, fallback stripes; no remote final artwork required to begin work. Poster palette extraction may fall back to fixed ink/paper; contrast>=4.5:1. Labels/alt text cannot contain hidden episode details.

Primitives owned by R04/R05: Button, TextField, TextArea, Checkbox, RadioGroup, Badge, Tabs, Progress, Skeleton, Poster, Avatar, Dialog, Sheet, Menu, Toast, EmptyState, ErrorState, SpoilerNotice. Feature components compose these; no second modal/toast system. Targets>=44px including desktop avatar (correct original42px). Focus3px accent+2px offset. Dialog traps focus/restores trigger, Escape closes cancellable overlays, accessible names mandatory; no silent discard of in-flight destructive mutation. Success toast6s with Undo, error persistent Retry/Dismiss, aria-live status/alert. Reduced motion disables animation. Keyboard-only and axe checks plus screenshots at320/390/860/1440.

## D02 — Prototype v2 route mapping

| Product route | Prototype query after `SceneCask Prototype v2.dc.html` | Required coverage |
|---|---|---|
| / | ?start=new | Welcome, create/signin links, no account-data sample injection |
| /auth/signup, /auth/signin | ?start=new&auth=create or signin | Validating, busy, field/form failure; add Google button |
| /auth/verify | ?start=new&auth=verify | Check inbox, resend cooldown, invalid/expired token; real link, no simulated verification button |
| /auth/reset, /auth/reset/confirm | Signin → forgot | Request, inbox, new-password form, success, expired/replayed link |
| /onboarding/profile | Supplement D03 | Name+unique handle; required before social use; skip allowed in alpha |
| /discover | ?start=returning&screen=discover&q=hollow | Search/results/disambiguation/loading/no-results/error; recommendations only beta |
| /home | ?start=returning&screen=home; start=caught; start=empty | Featured next, other shows, caught-up, no-watching versus empty-library, no-friends |
| /shows/[showId] | ?start=returning&screen=show&id=ho | Poster/info/status, season tabs, specials, episode rows, add/progress/remove/history |
| /shows/[showId] edge states | id=sk&s=2; id=hl | Undated, future, canceled incomplete, not available |
| /episodes/[episodeId] | ?start=returning&screen=episode&id=ho&s=2&e=8 | Protected/revealed details, mark/unmark, separate catch-up |
| /episodes/[episodeId] social | &beta=1; id=pm&s=1&e=3; reveal=disc | Hosted threads, gap lock, explicit scoped reveal, composer, participant labels |
| /library | ?start=returning&screen=library | Four manual-status filters, counts, search, empty/no matches, derived Completed |
| /friends | ?start=returning&beta=1&screen=friends | Search, incoming requests, pending/approved/public/private, no people/results/error |

IDs in prototype are fixture aliases only, not production identifiers. Query params are design fixture controls, not a production API. Alpha/Beta switch, simulated failures, preset users, sample date labels and metadata-release controls do not ship.

Corrections: remove manual Completed pill; allow future episode mark/unmark with future label; show catch-up exact included/excluded lists; use discussion-ID scoped reveals; real Google/email auth; report queue required; block has no Undo, settings offers unblock. Home visible text never uses episode titles for unwatched episodes. Browser Back/Forward and reload preserve routes, filters in query where appropriate, and account state via API.

## D03 — Account and SSO additions

Account forms: Google sign-in button above email/password fields separated by "or". Pending provider redirect disables duplicate submission; canceled/failed callback returns a form-level error with Retry and email alternative. "Account already exists" linkage screen says sign in using the existing method, then link Google in settings; no automatic merge. Error copy does not publicly confirm email existence. Verify/reset screens use same480px column, primary action plus resend/back link; expired token has explicit recovery.

Profile onboarding: display name and handle fields, format hint and availability/conflict inline error; Save/Skip in alpha. Social entry without handle routes here; initial handle becomes fixed after save. No avatar upload. Display preview initials.

## D04 — Settings/privacy (/settings)

Account-menu link; desktop760px column, mobile single stacked column. Sections: Profile, Sign-in methods, Privacy (beta), Blocked people (beta), Your data (beta), About. Section-rule3px ink, raised-paper rows, action button aligned right then wrapping below on mobile.

Profile: display-name edit + read-only handle (or choose if absent). Sign-in methods: email/password enabled status, link/unlink Google, set password via verified reset, sign out; unlink disabled if last method with explanatory text. Reauth dialog uses password or Continue with Google and returns to pending action.

Privacy radio group Private(default)/Public; Public confirmation: "Any signed-in SceneCask member can see your library and activity. Discussions still require mutual follows." Save shows busy, errors inline, success announced. Existing pending requests remain pending and visible in Friends. Followers list has Remove button+confirmation stating loss of access and no notification. Empty/loading/error/retry states mandatory. Blocked list includes Unblock confirmation stating follows will not return.

About includes TMDB attribution/logo/link and metadata availability disclaimer. No billing section.

## D05 — Notifications (/notifications, beta)

Header/account-menu entry with accessible unread count, no sixth mobile nav item.760px list with title Notifications; each row initials/name, "requested to follow you", relative date with accessible absolute date, unread dot plus text. Open goes to incoming Friends request; mark-read action. No episode titles/images/comment previews. Empty "You're all caught up"; loading skeleton; persistent load error Retry; request already resolved presents "This request is no longer pending." No email/push or reply notifications.

## D06 — Data controls (/settings/data, beta)

Export section explains own data, JSON,24h expiry. Export button→queued/running progress message→Download when ready; failed state offers retry; expired state offers new export. Poll only while in progress, cancel polling on leave. Download authenticated, no public URL.

Delete section visually separated with destructive outline action. Dialog explains permanent removal, loss of hosted discussions, bodyless anonymous comment placeholders, backups expire within30d. Require fresh reauth and typed DELETE; disabled until valid. Submit busy; rejected request remains recoverable; accepted request signs user out and shows deletion-in-progress confirmation. Do not claim immediate physical deletion; contract permits24h job completion. No Undo.

## D07 — Moderation (/operator/reports, beta)

Visible only to operators, but backend enforces role.760px report queue with Open/Resolved/Dismissed tabs; report row reason, timestamp, discussion/episode code, author context. Detail panel includes explicit "Reported content may contain spoilers" notice before operator opens body; this is moderation access, not user reveal. Actions Remove comment or Dismiss report, each confirmation and optional cancellation. Loading/empty/pagination/failure, concurrent-resolution conflict and forbidden states. Successful action advances queue and announces status. No role management, bans or automated classification UI.

## D08 — Social/profile supplement

/friends has Find people, Incoming requests and Followers tabs with44px targets; profile /people/[id] uses initials+name+handle, visibility badge and follow/request/cancel/unfollow/block actions. Private inaccessible library shows locked explanation without counts/progress; visible library uses reusable show rows with viewer spoiler policy. Public does not mean anonymous access. Before first discussion entry/post, text explains mutual followers of host may participate and see one another. No mutual connections → meaningful empty discussion list and Start your discussion.

Comment menu offers Delete(author), Remove(host), Report(other), Block(other); plain text<=5000, counter/error, posting pending retains draft on failure; successful post clears it. Removed comments use bodyless tombstone; hidden/blocked comments absent. Special episode discussion explicit warning every session before reveal. Gap notice lists codes only and makes no automatic progress changes.

## D09 — Required manual review evidence

For each UI issue: screenshots of happy path and its highest-risk state at390 and1440; check320 no overflow and860 nav switch; keyboard focus order, dialog focus, reduced motion; automated axe results and actual API network proof for integrated flows. Integration gates require fresh account and no friends, out-of-order tracking, concurrent Undo, metadata failure, revoked social access, and protected-payload inspection. Do not claim full visual verification from source inspection or one desktop screenshot.
