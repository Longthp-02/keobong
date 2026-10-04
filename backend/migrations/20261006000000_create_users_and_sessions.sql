-- Accounts, sign-in identities, server-side sessions and pending OAuth sign-ins.
-- Every match now has a host. No production data exists yet, so the column is
-- added as NOT NULL directly (local dev databases with matches must be reset).

CREATE TABLE users (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Public profile fields (shown to everyone, see spec.md "Public visibility").
    display_name text CHECK (char_length(display_name) BETWEEN 1 AND 80),
    avatar_url text CHECK (char_length(avatar_url) <= 500 AND avatar_url LIKE 'https://%'),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- One row per external account; a user may later link Google and Zalo.
CREATE TABLE user_identities (
    provider text NOT NULL CHECK (provider IN ('google', 'zalo')),
    subject text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 255),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Private: never part of any public view.
    email text CHECK (char_length(email) <= 320),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, subject)
);
CREATE INDEX user_identities_user_id_idx ON user_identities (user_id);

-- The cookie holds a random token; only its SHA-256 hash is stored, so a
-- database leak does not leak usable sessions.
CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at)
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

-- Short-lived state for one OAuth sign-in (state, PKCE verifier, nonce).
-- Rows are single use and expire after a few minutes.
CREATE TABLE oauth_login_attempts (
    state text PRIMARY KEY CHECK (char_length(state) BETWEEN 32 AND 64),
    code_verifier text NOT NULL CHECK (char_length(code_verifier) BETWEEN 43 AND 128),
    nonce text NOT NULL CHECK (char_length(nonce) BETWEEN 16 AND 64),
    return_to text NOT NULL CHECK (char_length(return_to) BETWEEN 1 AND 200 AND return_to LIKE '/%'),
    created_at timestamptz NOT NULL
);
CREATE INDEX oauth_login_attempts_created_at_idx ON oauth_login_attempts (created_at);

ALTER TABLE matches ADD COLUMN host_user_id bigint NOT NULL REFERENCES users (id);
CREATE INDEX matches_host_user_id_idx ON matches (host_user_id);
