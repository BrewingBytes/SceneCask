-- R02 0001: accounts, credentials, external identities, sessions and auth flows.
-- Secrets are stored only as hashes or ciphertext; never log these columns.

-- Shared guard: revision counters may only move forward (C02).
CREATE FUNCTION reject_revision_decrease() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    old_revision bigint;
    new_revision bigint;
BEGIN
    -- A missing or misspelled column argument would otherwise compare NULLs and pass silently.
    IF TG_NARGS <> 1 OR NOT to_jsonb(NEW) ? TG_ARGV[0] THEN
        RAISE EXCEPTION 'reject_revision_decrease needs an existing revision column argument'
            USING ERRCODE = 'undefined_column', TABLE = TG_TABLE_NAME;
    END IF;
    old_revision := to_jsonb(OLD) ->> TG_ARGV[0];
    new_revision := to_jsonb(NEW) ->> TG_ARGV[0];
    IF new_revision < old_revision THEN
        RAISE EXCEPTION 'revision must not decrease'
            USING ERRCODE = 'check_violation', TABLE = TG_TABLE_NAME, COLUMN = TG_ARGV[0];
    END IF;
    RETURN NEW;
END;
$$;

CREATE TABLE users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    normalized_email text NOT NULL UNIQUE
        CHECK (normalized_email <> '' AND normalized_email = btrim(normalized_email)
               AND normalized_email = lower(normalized_email)),
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
    -- Target for session-bound rows that also store the owner (reveal_grants).
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
