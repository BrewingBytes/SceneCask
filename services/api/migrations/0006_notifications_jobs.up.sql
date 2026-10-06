-- R02 0006: in-app notifications, transactional outbox and leased background jobs.
-- Payloads must never hold credentials, tokens or protected episode content.

CREATE TABLE notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('follow_request')),
    -- Nullable per C02 for future actorless kinds; a follow request always names its requester.
    actor_id uuid CHECK (kind <> 'follow_request' OR actor_id IS NOT NULL),
    -- The requester of a follow_request, NULL for every other kind, so the follow-edge rules
    -- below bind only follow requests and a future kind can name an actor freely. A future kind
    -- with an actor adds its own users reference for actor_id.
    follow_requester_id uuid GENERATED ALWAYS AS
        (CASE WHEN kind = 'follow_request' THEN actor_id END) STORED,
    created_at timestamptz NOT NULL DEFAULT now(),
    read_at timestamptz,
    -- A follow request notification lives as long as its follow edge: withdrawal, decline,
    -- follower removal, unfollow, block and either user's erasure delete the edge and the
    -- notification with it, so a later request can notify again. actor_id has no separate users
    -- reference: a SET NULL there could detach the row from its edge before the cascade.
    FOREIGN KEY (follow_requester_id, user_id)
        REFERENCES follows (follower_id, followee_id) ON DELETE CASCADE,
    -- One follow-request notification per requester and recipient, so follow retries cannot
    -- duplicate. Its index also serves the cascade from follows.
    CONSTRAINT notifications_follow_request_key UNIQUE (follow_requester_id, user_id)
);
CREATE INDEX notifications_user_idx ON notifications (user_id, created_at DESC, id DESC);

CREATE TABLE outbox (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind text NOT NULL CHECK (kind ~ '^[a-z][a-z_.]*$'),
    payload jsonb NOT NULL DEFAULT '{}',
    dedupe_key text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    available_at timestamptz NOT NULL DEFAULT now(),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    delivered_at timestamptz
);
CREATE INDEX outbox_pending_idx ON outbox (available_at) WHERE delivered_at IS NULL;

CREATE TABLE jobs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- SET NULL keeps the claimed deletion job's row so its outcome can be recorded and the job can
    -- be retried or recovered after a crash. Any other user job then fails the CHECK below, so
    -- erasure must first delete export files and rows.
    user_id uuid REFERENCES users (id) ON DELETE SET NULL,
    kind text NOT NULL CHECK (kind IN ('export', 'delete', 'metadata_refresh')),
    state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'ready', 'failed')),
    payload jsonb NOT NULL DEFAULT '{}',
    result_path text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    -- A crashed worker's claim expires here and the job becomes claimable again.
    lease_until timestamptz,
    CHECK (user_id IS NOT NULL OR kind IN ('metadata_refresh', 'delete')),
    -- A running job always holds a lease, so another worker cannot claim it immediately.
    CHECK (state <> 'running' OR lease_until IS NOT NULL)
);
-- The CHECK cannot tell a deletion job that lost its user to erasure from one that never had a
-- user. A delete job is created with its user and may only drop it once that user is gone, while
-- a worker has claimed it. Afterwards the userless job may change state freely, so lease recovery
-- and bounded retries can requeue it until its outcome is recorded.
CREATE FUNCTION jobs_delete_target_required() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' OR OLD.kind <> 'delete'
        OR (OLD.user_id IS NOT NULL
            AND (OLD.state <> 'running' OR EXISTS (SELECT FROM users WHERE id = OLD.user_id)))
    THEN
        RAISE EXCEPTION 'a deletion job needs its user until a claimed job erases that user'
            USING ERRCODE = 'check_violation', TABLE = TG_TABLE_NAME, COLUMN = 'user_id';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER jobs_delete_target_required
    BEFORE INSERT OR UPDATE ON jobs
    FOR EACH ROW WHEN (NEW.kind = 'delete' AND NEW.user_id IS NULL)
    EXECUTE FUNCTION jobs_delete_target_required();
-- One active export (and one active deletion) per user; repeats return the active job.
CREATE UNIQUE INDEX jobs_active_user_kind_key
    ON jobs (user_id, kind) WHERE kind IN ('export', 'delete') AND state IN ('queued', 'running');
CREATE INDEX jobs_claim_idx ON jobs (lease_until NULLS FIRST, created_at)
    WHERE state IN ('queued', 'running');
-- User erasure sets jobs.user_id to NULL; index it so the delete does not scan.
CREATE INDEX jobs_user_idx ON jobs (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX jobs_expires_idx ON jobs (expires_at) WHERE expires_at IS NOT NULL;
