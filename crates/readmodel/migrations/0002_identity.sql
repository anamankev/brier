-- Identity and audit for the gateway (docs/gates/G4-plan.md). A user's engine account ID is
-- the user's ID. Session tokens and API keys are never stored in a usable form: sessions keep
-- only the SHA-256 of the token, and API keys are Ed25519 public keys.

CREATE TABLE users (
    id bigserial PRIMARY KEY,
    username text NOT NULL UNIQUE CHECK (username ~ '^[a-z0-9_]{3,32}$'),
    password_hash text NOT NULL,               -- Argon2id, PHC string format
    role text NOT NULL DEFAULT 'trader' CHECK (role IN ('trader', 'admin')),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id bigint NOT NULL REFERENCES users (id),
    expires_at timestamptz NOT NULL
);
CREATE INDEX sessions_by_user ON sessions (user_id);

CREATE TABLE api_keys (
    id bigserial PRIMARY KEY,
    user_id bigint NOT NULL REFERENCES users (id),
    public_key bytea NOT NULL UNIQUE CHECK (length(public_key) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);

-- Every administrative command, with who, why, and what the engine answered.
CREATE TABLE audit_log (
    id bigserial PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    user_id bigint NOT NULL REFERENCES users (id),
    action text NOT NULL,
    detail jsonb NOT NULL,
    reason text NOT NULL CHECK (length(reason) BETWEEN 3 AND 500),
    seq bigint                                  -- the command's sequence number, if it was sequenced
);
