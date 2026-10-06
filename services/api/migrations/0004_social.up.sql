-- R02 0004: follow graph, blocks and activity. Activity never copies episode titles or text.

CREATE TABLE follows (
    follower_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    followee_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    state text NOT NULL CHECK (state IN ('pending', 'approved')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (follower_id, followee_id),
    CONSTRAINT follows_no_self CHECK (follower_id <> followee_id)
);
-- Incoming requests and followers lists, keyset createdAt,id descending.
CREATE INDEX follows_followee_idx ON follows (followee_id, state, created_at DESC, follower_id DESC);
-- Following lists, keyset createdAt,id descending.
CREATE INDEX follows_follower_idx ON follows (follower_id, state, created_at DESC, followee_id DESC);

CREATE TABLE blocks (
    blocker_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    blocked_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker_id, blocked_id),
    CONSTRAINT blocks_no_self CHECK (blocker_id <> blocked_id)
);
-- Blocks apply in both directions, so look up the reverse edge too.
CREATE INDEX blocks_blocked_idx ON blocks (blocked_id, blocker_id);

CREATE TABLE activity_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('library_added', 'episode_watched')),
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    episode_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- Undo deletes events created by the reversed action. Pruning expired actions keeps the
    -- event and clears the link.
    action_id uuid REFERENCES mutation_actions (id) ON DELETE SET NULL,
    CHECK ((kind = 'episode_watched') = (episode_id IS NOT NULL)),
    -- A watched event's episode belongs to the event's show.
    FOREIGN KEY (show_id, episode_id) REFERENCES episodes (show_id, id) ON DELETE RESTRICT
);
CREATE INDEX activity_events_actor_idx ON activity_events (actor_id, created_at DESC, id DESC);
CREATE INDEX activity_events_action_idx ON activity_events (action_id) WHERE action_id IS NOT NULL;
