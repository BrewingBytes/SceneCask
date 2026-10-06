-- R02 0003: library entries, per-episode progress and mutation provenance.
-- User-private rows cascade with the user; catalog references restrict so history is never
-- removed by catalog changes.

CREATE TABLE library_entries (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    saved boolean NOT NULL DEFAULT true,
    status text NOT NULL DEFAULT 'plan_to_watch'
        CHECK (status IN ('plan_to_watch', 'watching', 'on_hold', 'dropped')),
    saved_at timestamptz,
    -- No row reads as revision 0, so the first write stores 1 and mutation_changes.after_revision
    -- can always name it.
    revision bigint NOT NULL CHECK (revision > 0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, show_id),
    CHECK (NOT saved OR saved_at IS NOT NULL)
);
-- GET /library keyset: saved_at,id descending over saved entries.
CREATE INDEX library_entries_saved_idx
    ON library_entries (user_id, saved_at DESC, show_id DESC) WHERE saved;
-- Any change to the entry's state must advance its revision so expectedRevision checks and undo
-- see it; a revision may never move back.
CREATE TRIGGER library_entries_revision_forward
    BEFORE UPDATE ON library_entries
    FOR EACH ROW WHEN (NEW.revision < OLD.revision OR (NEW.revision = OLD.revision
        AND (NEW.saved, NEW.status, NEW.saved_at) IS DISTINCT FROM (OLD.saved, OLD.status, OLD.saved_at)))
    EXECUTE FUNCTION reject_revision_decrease('revision');

-- Aggregate tracking revision; progress mutations serialize on this row.
CREATE TABLE tracking_show_state (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    revision bigint NOT NULL DEFAULT 0 CHECK (revision >= 0),
    PRIMARY KEY (user_id, show_id)
);
CREATE TRIGGER tracking_show_state_revision_forward
    BEFORE UPDATE OF revision ON tracking_show_state
    FOR EACH ROW WHEN (NEW.revision < OLD.revision)
    EXECUTE FUNCTION reject_revision_decrease('revision');

CREATE TABLE mutation_actions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    kind text NOT NULL CHECK (kind ~ '^[a-z][a-z_]*$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    undo_until timestamptz NOT NULL,
    undone_at timestamptz,
    -- Stored undo outcome so a repeated undo returns the same result.
    undo_result jsonb,
    CHECK (undo_until > created_at),
    CHECK ((undone_at IS NULL) = (undo_result IS NULL))
);
CREATE INDEX mutation_actions_user_idx ON mutation_actions (user_id, created_at DESC);

CREATE TABLE mutation_changes (
    action_id uuid NOT NULL REFERENCES mutation_actions (id) ON DELETE CASCADE,
    entity text NOT NULL CHECK (entity IN ('episode_progress', 'library_entry')),
    entity_key text NOT NULL,
    field text NOT NULL,
    before_value jsonb NOT NULL,
    after_revision bigint NOT NULL CHECK (after_revision > 0),
    PRIMARY KEY (action_id, entity, entity_key, field)
);

-- False rows are kept: they carry revision provenance for later writes and undo.
CREATE TABLE episode_progress (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    episode_id uuid NOT NULL REFERENCES episodes (id) ON DELETE RESTRICT,
    watched boolean NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    last_action_id uuid REFERENCES mutation_actions (id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, episode_id)
);
CREATE INDEX episode_progress_action_idx ON episode_progress (last_action_id)
    WHERE last_action_id IS NOT NULL;
-- Every watched change, including true→false→true, advances the revision. Clearing
-- last_action_id when undo actions are pruned does not.
CREATE TRIGGER episode_progress_revision_forward
    BEFORE UPDATE ON episode_progress
    FOR EACH ROW WHEN (NEW.revision < OLD.revision
        OR (NEW.revision = OLD.revision AND NEW.watched IS DISTINCT FROM OLD.watched))
    EXECUTE FUNCTION reject_revision_decrease('revision');

CREATE TABLE idempotency_records (
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key uuid NOT NULL,
    request_hash bytea NOT NULL,
    response jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, key)
);
CREATE INDEX idempotency_records_expires_idx ON idempotency_records (expires_at);

CREATE TABLE catchup_previews (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    endpoint_episode_id uuid NOT NULL,
    episode_ids jsonb NOT NULL CHECK (jsonb_typeof(episode_ids) = 'array'),
    revisions jsonb NOT NULL CHECK (jsonb_typeof(revisions) = 'object'),
    catalog_revision bigint NOT NULL CHECK (catalog_revision >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    CHECK (expires_at > created_at),
    -- The endpoint episode belongs to the previewed show.
    FOREIGN KEY (show_id, endpoint_episode_id) REFERENCES episodes (show_id, id) ON DELETE RESTRICT
);
CREATE INDEX catchup_previews_user_idx ON catchup_previews (user_id, show_id);
CREATE INDEX catchup_previews_expires_idx ON catchup_previews (expires_at);
