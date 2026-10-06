-- Development/test fixture only. Never load into a real deployment or real accounts.
-- Credential values are inert placeholders, not real hashes.
INSERT INTO users (id, normalized_email, display_name, handle, visibility, verified_at, role)
VALUES
    ('00000000-0000-4000-8000-00000000a0a1', 'ana@scenecask.test', 'Ana', 'ana_r', 'private', now(), 'member'),
    ('00000000-0000-4000-8000-00000000b0b1', 'ben@scenecask.test', 'Ben', 'ben_k', 'public', now(), 'member'),
    ('00000000-0000-4000-8000-00000000c0c1', 'cleo@scenecask.test', 'Cleo', 'cleo_m', 'private', now(), 'operator');

INSERT INTO password_credentials (user_id, argon2_hash)
VALUES ('00000000-0000-4000-8000-00000000a0a1', '$argon2id$v=19$m=19456,t=2,p=1$fixture$fixture');

INSERT INTO external_identities (user_id, issuer, subject)
VALUES ('00000000-0000-4000-8000-00000000b0b1', 'https://accounts.google.com', 'fixture-subject-ben');
