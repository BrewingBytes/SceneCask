# Spec review: prototype v1 → v2

Source of truth: the SceneCask product specification.
- **Built to the spec's confirmed decisions:** items marked (C).
- **Built to the spec's recommended defaults:** items marked (R). These are shown as proposals and are not approved.

## Conflicts found in v1, and what v2 does instead

| # | v1 behavior | Spec | v2 |
|---|---|---|---|
| 1 | Undo restored a full snapshot of the show, which could overwrite later edits | Undo reverses only that action's changes; later conflicting edits are not overwritten (R) | Each action records its own per-field changes. Undo and failure rollback revert only fields still holding the action's value, and the toast reports any kept changes |
| 2 | Discussion = comments from anyone you follow | Discussions are hosted; access is the host plus the host's mutual approved follows; participants see each other (C) | Each episode lists the hosted discussions you can open, and you can start your own. Comments are labeled Host / You follow / Participant. Non-mutual hosts (Priya) aren't listed |
| 3 | One reveal opened details and discussion together, with no time limit | Reveal names its scope: episode details, or this discussion; lasts for the session (R) | Two separate reveals, each with a confirmation naming its scope. They aren't saved, so they end when the session ends. Neither changes progress |
| 4 | "Completed series" = ended + all watched | Completed also requires no unresolved release dates; Completed outranks Caught up (C) | Undated episodes block Completed (Saltwater Kings is canceled with S2 E5 undated, so it stays Caught up) |
| 5 | No undated-episode state; unaired episodes couldn't be marked | Undated: never assumed released or suggested as next, explicit individual mark allowed, uncertainty notice (R) | `?` dashed ring that can still be marked, a "Release info incomplete" notice, and exclusion from next episode and catch-up |
| 6 | Catch-up included every regular episode through the endpoint | Released regular episodes only; specials, future and undated excluded and named in the confirmation (C/R) | Dialog and sheet list the range, the count, and "Not included: …" |
| 7 | No "nothing released" state | Use "Not yet available"; "Release information unknown" if availability can't be established (C/R) | Harbor Lights premieres Thu, Nov 12 and shows "Not yet available" |
| 8 | Social features were part of the main flow | Personal tracking alpha ships before social beta (R) | Alpha/Beta switch in the account menu. Alpha (default) hides Friends, friend activity, discussions and friend recommendations |
| 9 | Following was always approval-based | Private by default; public accounts can be followed immediately (C) | Jonas is a public account and can be followed immediately; the others need approval. Pending requests can be withdrawn |
| 10 | Blocking hid only the other person's comments for you | Blocking removes follows and requests in both directions, and hides each person's content from the other in shared discussions (R) | Both follow directions and requests are removed, and the copy says so |
| 11 | Unread comments' activity said "Open discussion" | Activity previews omit protected text (C) | Activity shows only episode codes and "details hidden until you've watched through Sx Ey" |

## Added because the alpha requires it
- **Account access (C):** create account (validation, provider error), email verification with resend, sign in, password reset request and "check your inbox", sign out.
- **Persistent progress (C):** the library and progress are stored locally in the prototype and survive reloads. Sign out and sign in again restores them.
- **Out-of-order viewing (C):** Paper Moons has E1 and E3 watched, so next is E2, labeled "some out of order".
- **Library removal (R):** removing keeps history. "Add back" restores progress. "Erase watch history" is a separate confirmed action with undo.
- **Release boundary note (R):** "Release dates come from TV listings, at midnight in the show's home time zone. Streaming services may differ."
- **Metadata change (R):** a prototype control releases Night Shift S4 E12, which moves the show from Caught up back to Up next.
- **Specials (C):** specials discussions always need an explicit warning (beta). Specials are excluded from progress, next episode, catch-up and completion.
- **Author deletion and host removal (R, beta):** in the ⋯ menu on a comment.

## Kept, but flagged for product review
- **Library statuses** (Plan to watch, Watching, On hold, Dropped, Completed) came from the original brief and aren't in the spec. A user can set "Completed" while the derived state says otherwise. The library shows both: the status badge and the derived progress text.
- **Comment reactions** (beta) aren't in the spec.

## Not built (out of first-release scope, or not specified)
- Settings: switching to a public account (with its warning), removing followers.
- In-app notifications for follow requests and replies.
- Data export and account deletion.
- Operator moderation review queue.
- All spec non-goals: streaming, movies, alternative viewing orders, public forums, DMs.
