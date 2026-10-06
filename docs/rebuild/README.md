# SceneCask rebuild

This is a fresh implementation backlog derived from the user's approved decisions and supplied Claude Design export. Existing GitHub issues were not read or used as requirements. No application has been implemented.

Read in order:

1. [Decisions](decisions.md)
2. [Architecture](architecture.md)
3. [Canonical contracts](contracts.md)
4. [Design supplement](design/supplement.md) and [source handoff](design/source/handoff/README.md)
5. [Backlog](backlog.md) and [coverage](coverage.md)

Start with R01. Every issue's prerequisites must be merged before implementation. R02 owns initial migrations and R03 owns the API schema/generated-client contract. R24 and R38 own shared integration wiring. Do not reinterpret historical prototype behavior that conflicts with decisions/contracts.

## Reusable agent prompt

Implement only ISSUE_URL. Read its body, exact prerequisites, repository AGENTS.md, and the pinned canonical contracts/design references before editing. Confirm prerequisites are merged. Respect assigned file ownership; you are not alone in the repository, so do not revert unrelated work. Coordinate changes to shared contracts, migrations, manifests and routers. Implement the assigned outcome, including failure/privacy/spoiler cases; run the required checks and browser verification. Open one reviewable PR and report changed files, commands actually run, results, relevant screenshots, limitations and follow-up dependencies. Do not call mock-only frontend work integrated. If a real contract conflict appears, report it rather than inventing incompatible behavior.
