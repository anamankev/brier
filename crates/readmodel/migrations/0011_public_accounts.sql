-- Public accounts (G10 U1): the age and terms each user confirmed at sign-up, recovery codes
-- for lost passwords, sign-up limits per client address, and account deletion.

ALTER TABLE users
    ADD COLUMN adult_confirmed_at timestamptz,          -- the user confirmed they are 18 or older
    ADD COLUMN terms_version text,                      -- the terms the user accepted
    ADD COLUMN terms_accepted_at timestamptz,
    ADD COLUMN signup_address bytea CHECK (signup_address IS NULL OR length(signup_address) = 32),
    ADD COLUMN deleted_at timestamptz;                  -- personal data removed; the row stays for the ledger
CREATE INDEX users_by_signup_address ON users (signup_address, created_at) WHERE signup_address IS NOT NULL;
CREATE INDEX users_by_created_at ON users (created_at);

-- One-time codes that reset a lost password. Only the SHA-256 of each code is kept: a code
-- carries 80 random bits, so a fast hash is enough (as for session tokens).
CREATE TABLE recovery_codes (
    user_id bigint NOT NULL REFERENCES users (id),
    code_hash bytea NOT NULL CHECK (length(code_hash) = 32),
    used_at timestamptz,
    PRIMARY KEY (user_id, code_hash)
);
