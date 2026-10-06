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
-- The 12-hour cap is measured from created_at, so the database stamps it from its own wall clock
-- on every insert or created_at change. A value bound by the writer is ignored, so neither a
-- future created_at nor app-host clock skew can affect the cap. Writers derive expires_at in SQL
-- (now() + interval '12 hours' or less), which this stamp never precedes.
CREATE FUNCTION reveal_grants_stamp_created_at() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.created_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER reveal_grants_stamp_created_at
    BEFORE INSERT OR UPDATE OF created_at ON reveal_grants
    FOR EACH ROW EXECUTE FUNCTION reveal_grants_stamp_created_at();
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
-- Operator erasure sets operator_id to NULL; index it so the delete does not scan.
CREATE INDEX moderation_audit_operator_idx ON moderation_audit (operator_id)
    WHERE operator_id IS NOT NULL;
