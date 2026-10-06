-- R02 0007: session-scoped spoiler reveals and the operator moderation audit.

CREATE TABLE reveal_grants (
    -- Logout, reset and session expiry delete the session and revoke its grants.
    session_id bytea NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scope text NOT NULL CHECK (scope IN ('episode_details', 'discussion')),
    resource_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (session_id, scope, resource_id),
    -- The grant's user must own the session, so revoking by user_id reaches every grant.
    FOREIGN KEY (session_id, user_id) REFERENCES sessions (id_hash, user_id) ON DELETE CASCADE,
    CHECK (expires_at > created_at AND expires_at <= created_at + interval '12 hours')
);
-- Blocks and unfollows revoke a user's discussion grants.
CREATE INDEX reveal_grants_user_idx ON reveal_grants (user_id, scope, resource_id);

CREATE TABLE moderation_audit (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- NULL after operator erasure; history keeps only anonymous action identifiers.
    operator_id uuid REFERENCES users (id) ON DELETE SET NULL,
    report_id uuid NOT NULL REFERENCES reports (id) ON DELETE RESTRICT,
    action text NOT NULL CHECK (action IN ('view', 'remove_comment', 'dismiss')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX moderation_audit_report_idx ON moderation_audit (report_id, created_at);
