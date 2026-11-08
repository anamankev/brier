-- Surveillance (SPEC §9; docs/surveillance.md).
-- A fourth role: the surveillance service reads login links and opens cases, and nothing else.
ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('trader', 'admin', 'resolver', 'surveillance'));

-- One row per user and client address seen at login. The address is never stored: only an
-- HMAC-SHA-256 of it under a key derived from BRIER_TOTP_KEY, so two users who share an
-- address can be linked, and the address cannot be read back.
CREATE TABLE login_signals (
    user_id bigint NOT NULL REFERENCES users (id),
    address_hash bytea NOT NULL CHECK (length(address_hash) = 32),
    first_seen timestamptz NOT NULL DEFAULT now(),
    last_seen timestamptz NOT NULL DEFAULT now(),
    logins integer NOT NULL DEFAULT 1 CHECK (logins > 0),
    PRIMARY KEY (user_id, address_hash)
);
CREATE INDEX login_signals_by_address ON login_signals (address_hash);

-- One case per (typology, market, account): an alert from a detector, worked by an admin.
CREATE TABLE cases (
    id bigserial PRIMARY KEY,
    typology text NOT NULL CHECK (typology IN ('wash', 'spoof', 'close', 'insider')),
    market bigint NOT NULL CHECK (market >= 0),
    account bigint NOT NULL CHECK (account >= 0),
    score double precision NOT NULL CHECK (score > 0),
    detector text NOT NULL CHECK (length(detector) BETWEEN 1 AND 100),
    evidence jsonb NOT NULL,
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'dismissed', 'actioned')),
    opened_by bigint NOT NULL REFERENCES users (id),
    opened_at timestamptz NOT NULL DEFAULT now(),
    closed_by bigint REFERENCES users (id),
    closed_at timestamptz,
    close_reason text,
    UNIQUE (typology, market, account),
    CONSTRAINT cases_closed_has_closer CHECK (
        (status = 'open') = (closed_by IS NULL AND closed_at IS NULL AND close_reason IS NULL)
    )
);
CREATE INDEX cases_open ON cases (opened_at) WHERE status = 'open';
