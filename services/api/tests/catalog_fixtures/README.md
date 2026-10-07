# TMDB TV response fixtures

These checked-in synthetic responses record the endpoint shapes from TMDB's
official TV search, TV details and season-details documentation (checked
2026-10-07). They are not captures from a credentialed TMDB request. Names, IDs,
protected-string sentinels and paths are test-only, with no real accounts, payloads
or credentials. Provider tests replay them through a local HTTP server and inspect
the outbound TV endpoint/query/bearer behavior, mapping and failure redaction.
PostgreSQL/handler tests use the same modeled catalog at the `TvProvider` boundary.

Cases include ambiguous titles with different year/genre metadata, missing poster,
empty first-air date, an unusable result and a malformed date (skipped and
nulled), unknown extra fields, specials, null episode date, future
episode, missing details and protected episode title/overview/still sentinels.
Configuration and genre-list fixtures, 429/5xx/timeout/malformed/oversized/incomplete
responses are supplied by the HTTP harness in provider/tmdb.rs. No external
provider credentials are used by normal tests.
