-- R02 0005: host-owned episode discussions, comments, reports and per-viewer hiding.
-- Comments are shared content: user erasure anonymizes them and never cascades other
-- users' comments away.

CREATE TABLE discussions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- NULL after host erasure: the thread becomes inaccessible to everyone.
    host_id uuid REFERENCES users (id) ON DELETE SET NULL,
    episode_id uuid NOT NULL REFERENCES episodes (id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (host_id, episode_id)
);
CREATE INDEX discussions_episode_idx ON discussions (episode_id, created_at DESC, id DESC);

CREATE TABLE comments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    discussion_id uuid NOT NULL REFERENCES discussions (id) ON DELETE RESTRICT,
    author_id uuid REFERENCES users (id) ON DELETE SET NULL,
    body text CHECK (char_length(body) BETWEEN 1 AND 5000),
    created_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz,
    removed_by uuid REFERENCES users (id) ON DELETE SET NULL,
    -- A removed comment is a bodyless tombstone; a live comment always has a body.
    CONSTRAINT comments_tombstone CHECK ((body IS NULL) = (removed_at IS NOT NULL)),
    -- Erasing the author (SET NULL) fails until the comment is tombstoned (C08).
    CONSTRAINT comments_anonymous_tombstone CHECK (author_id IS NOT NULL OR body IS NULL)
);
-- GET /discussions/{id}/comments keyset: createdAt,id ascending.
CREATE INDEX comments_discussion_idx ON comments (discussion_id, created_at, id);
CREATE INDEX comments_author_idx ON comments (author_id) WHERE author_id IS NOT NULL;
-- User erasure sets these references to NULL; index them so the delete does not scan.
CREATE INDEX comments_removed_by_idx ON comments (removed_by) WHERE removed_by IS NOT NULL;

CREATE TABLE reports (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- NULL after reporter erasure; the report stays for moderation history.
    reporter_id uuid REFERENCES users (id) ON DELETE SET NULL,
    comment_id uuid NOT NULL REFERENCES comments (id) ON DELETE RESTRICT,
    reason text NOT NULL CHECK (reason IN ('later_episode', 'abuse', 'spam', 'other')),
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'resolved', 'dismissed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    reviewed_by uuid REFERENCES users (id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    CHECK ((state = 'open') = (reviewed_at IS NULL))
);
CREATE UNIQUE INDEX reports_open_reporter_comment_key
    ON reports (reporter_id, comment_id) WHERE state = 'open';
-- Operator queue keyset by state, createdAt,id descending; resolution looks up by comment.
CREATE INDEX reports_state_idx ON reports (state, created_at DESC, id DESC);
CREATE INDEX reports_comment_idx ON reports (comment_id);
CREATE INDEX reports_reporter_idx ON reports (reporter_id) WHERE reporter_id IS NOT NULL;
CREATE INDEX reports_reviewed_by_idx ON reports (reviewed_by) WHERE reviewed_by IS NOT NULL;

CREATE TABLE hidden_comments (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    comment_id uuid NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, comment_id)
);
