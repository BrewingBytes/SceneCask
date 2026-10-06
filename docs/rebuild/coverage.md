# Coverage and coordination map

Keys resolve to linked GitHub issues in backlog.md after publication. All prerequisites must be merged; same wave may run in parallel only within declared ownership. R02 is migration owner, R03 contract owner. Changes to shared manifests/lockfiles/router wiring outside ownership require coordination; no parallel ad-hoc edits. Integration owners R24/R38 assemble shared routes and feature slots. Every issue has a ready-to-use execution prompt and completion handoff.

| Journey / design state | Implementation issues | End-to-end evidence |
|---|---|---|
| Workspace, health, DB constraints, CI, generated client | R01 R02 R03 | R24 R39 R40 |
| Bright editorial tokens, responsive shell, keyboard/overlays/feedback | R04 R05 | R24 R38 R40 |
| Welcome, registration, email verification/resend/errors | R06 R07 R10 R23 | R24 |
| Login/logout/session expiry/reset/recovery | R06 R08 R10 | R24 |
| Google sign-in/cancel/linking/unlink/last method/reauth | R09 R10 R29 | R24 R38 R40 |
| Profile name/handle and social entry | R10 R15 R29 | R24 R38 |
| Search, ambiguous titles, pagination, no results/error | R11 R19 | R24 |
| Metadata import, missing art, provider outage/refresh/renumber | R11 R12 R18 | R24 R40 |
| Add to library, status/filter/count/search/empty/no matches | R15 R19 R20 | R24 |
| Remove/readd retains history; separate erase with Undo | R15 R16 R21 | R24 |
| Show, seasons, specials, future/undated/not-yet/completion | R13 R18 R21 | R24 |
| Individual watch/unwatch including future and out-of-order | R13 R16 R21 R22 R23 | R24 |
| Exact catch-up preview, exclusions, stale scope, no-op, Undo | R16 R17 R21 | R24 |
| Conflict-safe Undo and optimistic rollback/idempotency | R15 R16 R17 R21 | R24 R38 |
| Home next/caught-up/no-watching/no-friends/new release | R12 R13 R18 R23 | R24 |
| Hidden episode title/still/overview, reveal/hide/session revocation | R14 R18 R22 | R24 R38 |
| People search/profile, private/public, pending/cancel/approve/decline | R25 R28 | R38 |
| Followers/removal, public warning, private switch, block/unblock | R26 R28 R29 | R38 |
| Friend activity/recommendations, no-friends and revoked visibility | R27 R28 | R38 |
| Hosted thread list/start, mutual access and participant visibility | R14 R30 R32 | R38 |
| Discussion gap/special lock, separate thread reveal | R14 R18 R30 R32 | R38 |
| Post/retain draft/failure, author delete, host remove | R31 R32 | R38 |
| Report/hide, operator review, member forbidden, resolution race | R31 R33 | R38 R40 |
| Follow-request notifications/read/empty/resolved/revoked | R34 | R38 |
| Export queue/download/expiry/error/ownership | R35 R37 | R38 |
| Delete confirmation/reauth/disable/cleanup/tombstones | R36 R37 | R38 R39 |
| API/SSR/prefetch/activity/notification spoiler absence | R14 R18 R27 R30 R34 | R24 R38 R40 |
| Containers/SMTP/provider setup/TLS/backups/restore/worker alerts | R39 | R40 |

Source gallery: all20 mobile and4 desktop frames are covered by rows above. The obsolete manual Completed pill, future-mark restriction, block Undo, global episode discussion reveal and prototype controls are deliberately replaced per decisions/supplement. No final art is supplied: production TMDB artwork with accessible fallbacks is assigned R11/R04/R18, not left as a missing asset task.

Original missing settings/privacy, SSO, notifications, export/deletion and moderation screens are specified in design/supplement.md and assigned above. No reply/email/push social notifications, reactions, DMs, playback, native app, billing, movies, watch parties or AI recommendation infrastructure. Those are explicit non-goals, not unassigned required work.

## Milestones in order

1. Foundation: R01–R05.
2. Accounts: R06–R10.
3. Personal tracking: R11–R23.
4. Alpha acceptance: R24; solo application must work against actual API.
5. Social/discussions/account lifecycle: R25–R37 (dependency waves specify safe overlaps).
6. Beta acceptance: R38; multi-user privacy/spoiler checks against actual API.
7. Release preparation and acceptance: R39–R40.

## Deployment inputs, not product-design blockers

Google OAuth client/redirect configuration, TMDB API access appropriate to use, SMTP configuration, deployment domain/TLS and PostgreSQL credentials are supplied at deployment. No secrets are requested in issue bodies. R39 documents setup; R40 cannot claim real-provider smoke or production readiness without evidence. This planning task does not deploy or implement the application.
