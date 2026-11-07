-- TOTP second factor (RFC 6238; crates/gateway/src/totp.rs). The secret is stored encrypted
-- (nonce and ciphertext) under BRIER_TOTP_KEY. `totp_last_step` is the last 30-second step a
-- code was accepted for, so each code works once. A session remembers whether its login used
-- TOTP; admin actions need one that did.
ALTER TABLE users ADD COLUMN totp_secret bytea CHECK (length(totp_secret) = 36);
ALTER TABLE users ADD COLUMN totp_enabled boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN totp_last_step bigint;
ALTER TABLE users ADD CONSTRAINT users_totp_enabled_has_secret CHECK (NOT totp_enabled OR totp_secret IS NOT NULL);
ALTER TABLE sessions ADD COLUMN mfa boolean NOT NULL DEFAULT false;
