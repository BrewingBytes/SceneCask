-- R02 0006: in-app notifications, transactional outbox and leased background jobs.
-- Payloads must never hold credentials, tokens or protected episode content.

CREATE TABLE notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('follow_request')),
    actor_id uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    read_at timestamptz,
    -- A follow request notification lives as long as its follow edge: withdrawal, decline,
    -- follower removal, unfollow and block delete the edge and the notification with it, so a
    -- later request can notify again.
    FOREIGN KEY (actor_id, user_id) REFERENCES follows (follower_id, followee_id) ON DELETE CASCADE
);
CREATE INDEX notifications_user_idx ON notifications (user_id, created_at DESC, id DESC);
-- One notification per recipient, kind and actor: follow retries cannot duplicate.
CREATE UNIQUE INDEX notifications_user_kind_actor_key ON notifications (user_id, kind, actor_id)
    WHERE actor_id IS NOT NULL;
CREATE INDEX notifications_actor_idx ON notifications (actor_id, user_id) WHERE actor_id IS NOT NULL;

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
    -- SET NULL keeps the claimed deletion job's row so its outcome can be recorded. Any other
    -- user job then fails the CHECK below, so erasure must first delete export files and rows.
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
    CHECK (user_id IS NOT NULL OR kind = 'metadata_refresh' OR (kind = 'delete' AND state <> 'queued')),
    -- A running job always holds a lease, so another worker cannot claim it immediately.
    CHECK (state <> 'running' OR lease_until IS NOT NULL)
);
-- One active export (and one active deletion) per user; repeats return the active job.
CREATE UNIQUE INDEX jobs_active_user_kind_key
    ON jobs (user_id, kind) WHERE kind IN ('export', 'delete') AND state IN ('queued', 'running');
CREATE INDEX jobs_claim_idx ON jobs (lease_until NULLS FIRST, created_at)
    WHERE state IN ('queued', 'running');
-- User erasure sets jobs.user_id to NULL; index it so the delete does not scan.
CREATE INDEX jobs_user_idx ON jobs (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX jobs_expires_idx ON jobs (expires_at) WHERE expires_at IS NOT NULL;
