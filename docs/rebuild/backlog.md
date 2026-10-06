# Implementation backlog

40 scoped issues, pending GitHub publication. Product contracts and design references are settled.

| Key | Outcome | Prerequisites | Parallel group |
|---|---|---|---|
| R01 | Bootstrap the web/API workspace and repeatable CI | None | G01 |
| R02 | Create the canonical PostgreSQL schema and migration registry | R01 | G02 |
| R03 | Encode and validate the canonical OpenAPI and generated client | R01 | G02 |
| R04 | Build design tokens, navigation shell and basic primitives | R01 | G02 |
| R05 | Implement accessible sheets, dialogs, menus and feedback | R04 | G03 |
| R06 | Implement cookie sessions, CSRF and API error middleware | R02, R03 | G03 |
| R11 | Implement TMDB TV search and atomic catalog import | R02, R03 | G03 |
| R13 | Implement pure per-episode progress and release rules | R02 | G03 |
| R07 | Implement registration, verification and SMTP outbox delivery | R06 | G04 |
| R12 | Refresh metadata without losing watch history | R11 | G04 |
| R14 | Implement reusable privacy and spoiler projection policies | R02, R13 | G04 |
| R15 | Implement personal library and profile persistence APIs | R06, R13 | G04 |
| R08 | Implement password login, recovery and fresh reauthentication | R07 | G05 |
| R16 | Persist individual watched changes and conflict-safe Undo | R15 | G05 |
| R09 | Implement Google sign-in and explicit identity linking | R08 | G06 |
| R17 | Implement explicit catch-up preview and atomic commit | R16, R12 | G06 |
| R18 | Serve viewer-filtered show, episode, Home and reveal APIs | R11, R14, R16 | G06 |
| R19 | Build TV search, disambiguation and add-to-library flow | R05, R11, R15, R16 | G06 |
| R10 | Build account, recovery and Google sign-in screens | R05, R09, R15 | G07 |
| R20 | Build searchable library with derived progress states | R05, R15, R18 | G07 |
| R21 | Build show detail, episode rows and explicit catch-up sheet | R05, R17, R18 | G07 |
| R22 | Build spoiler-safe episode detail and scoped reveal UI | R21 | G08 |
| R23 | Build personal Home with next episode and caught-up sections | R21 | G08 |
| R24 | Integrate and verify the complete personal-tracking alpha | R10, R12, R17, R18, R19, R20, R21, R22, R23 | G09 |
| R25 | Implement follows, requests and visible people/library APIs | R24 | G10 |
| R35 | Implement private asynchronous JSON data export | R24 | G10 |
| R26 | Implement privacy changes and bilateral blocking | R25 | G11 |
| R27 | Generate spoiler-safe activity and friend recommendations | R26 | G12 |
| R29 | Build settings, privacy and sign-in method management | R10, R26 | G12 |
| R30 | Implement hosted discussions and gated comment reads | R26, R18 | G12 |
| R28 | Build Friends, profiles and social discovery surfaces | R27 | G13 |
| R31 | Implement comment posting, removal and reporting | R30 | G13 |
| R32 | Build hosted discussion selection, reveal and comments UI | R22, R31 | G14 |
| R33 | Implement operator report review and audit UI | R31, R29 | G14 |
| R34 | Implement in-app follow-request notifications | R25, R28 | G14 |
| R36 | Implement irreversible account deletion and cleanup | R35, R31, R34 | G15 |
| R37 | Build export and account deletion settings flows | R29, R36 | G16 |
| R38 | Integrate social beta and verify privacy/spoiler journeys | R28, R29, R32, R33, R34, R37 | G17 |
| R39 | Package deployment and document operations/recovery | R24, R38 | G18 |
| R40 | Run release acceptance and publish a go/no-go report | R39 | G19 |
