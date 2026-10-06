-- R02 0001: accounts, credentials, external identities, sessions and auth flows.
-- Secrets are stored only as hashes or ciphertext; never log these columns.

-- Shared guard for rules a CHECK cannot express because they compare OLD and NEW. Each trigger
-- states its rule in a WHEN clause, so PostgreSQL validates the column names at CREATE TRIGGER and
-- the function only runs for a rejected update. Arguments: the column, then the error message.
CREATE FUNCTION reject_update() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '%', TG_ARGV[1]
        USING ERRCODE = 'check_violation', TABLE = TG_TABLE_NAME, COLUMN = TG_ARGV[0];
END;
$$;

CREATE TABLE users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    normalized_email text NOT NULL UNIQUE
        -- Stored already trimmed and Unicode casefolded (C03). pg_unicode_fast gives full casefolding
        -- (for example 'ß' to 'ss') regardless of the database collation; the API must normalize with
        -- the same full casefold.
        CHECK (normalized_email <> '' AND normalized_email = btrim(normalized_email)
               AND normalized_email = casefold(normalized_email COLLATE pg_unicode_fast)),
    display_name text CHECK (char_length(display_name) BETWEEN 1 AND 80),
    handle text UNIQUE CHECK (handle ~ '^[a-z0-9_]{3,30}$'),
    visibility text NOT NULL DEFAULT 'private' CHECK (visibility IN ('private', 'public')),
    verified_at timestamptz,
    role text NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'operator')),
    disabled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE password_credentials (
    user_id uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    argon2_hash text NOT NULL CHECK (argon2_hash LIKE '$argon2id$%'),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE external_identities (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issuer text NOT NULL CHECK (issuer <> ''),
    subject text NOT NULL CHECK (subject <> ''),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (issuer, subject),
    -- One linked identity per provider per account (GET /me/identities is per provider).
    UNIQUE (user_id, issuer)
);

CREATE TABLE sessions (
    id_hash bytea PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    reauthenticated_at timestamptz,
    CHECK (expires_at > created_at),
    -- Target for session-bound rows that also store the owner (reveal_grants). A composite
    -- foreign key needs a unique constraint on exactly these columns, so the primary key
    -- alone cannot serve it.
    UNIQUE (id_hash, user_id)
);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

CREATE TABLE auth_tokens (
    token_hash bytea PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose text NOT NULL CHECK (purpose IN ('verify', 'reset')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CHECK (expires_at > created_at)
);
-- Invalidating prior tokens and the resend cooldown look up by user and purpose.
CREATE INDEX auth_tokens_user_purpose_idx ON auth_tokens (user_id, purpose, created_at DESC);

CREATE TABLE oauth_flows (
    state_hash bytea PRIMARY KEY,
    nonce_hash bytea NOT NULL,
    pkce_verifier_ciphertext bytea NOT NULL,
    intent text NOT NULL CHECK (intent IN ('signin', 'link', 'reauth')),
    session_id bytea REFERENCES sessions (id_hash) ON DELETE CASCADE,
    -- Same-origin path: no '//' or '/\' prefix, and no backslash, whitespace or control
    -- characters anywhere, since browsers strip tabs/newlines and could rebuild a '//' prefix.
    return_to text NOT NULL DEFAULT '/home'
        CHECK (return_to ~ '^/([^/\\]|$)' AND return_to !~ '[\\[:space:][:cntrl:]]'),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CHECK (expires_at > created_at),
    -- Link and reauth always act on the current authenticated session.
    CHECK (intent = 'signin' OR session_id IS NOT NULL)
);
CREATE INDEX oauth_flows_expires_idx ON oauth_flows (expires_at);
-- Session deletion cascades here; index it so logout and expiry sweeps do not scan.
CREATE INDEX oauth_flows_session_idx ON oauth_flows (session_id) WHERE session_id IS NOT NULL;
