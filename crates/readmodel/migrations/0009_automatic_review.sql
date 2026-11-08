-- Automatic review (G9; docs/rulebook.md schema 2). A resolution of a market whose rulebook
-- says review = automatic is confirmed by an independent re-fetch that computes the same
-- outcome (`verified_by`, the re-fetch's evidence record) instead of a second admin. A re-fetch
-- that disagrees is kept (`verify_mismatch`), and from then on only an admin can confirm.
ALTER TABLE resolutions ADD COLUMN verified_by bytea REFERENCES evidence (evidence_hash);
ALTER TABLE resolutions ADD COLUMN verify_mismatch bytea REFERENCES evidence (evidence_hash);
ALTER TABLE resolutions DROP CONSTRAINT resolutions_check2;
ALTER TABLE resolutions DROP CONSTRAINT resolutions_check3;
ALTER TABLE resolutions ADD CONSTRAINT resolutions_confirmation_complete CHECK (
    (confirmed_at IS NULL) = (dispute_until IS NULL)
    AND (confirmed_at IS NULL) = (confirmed_by IS NULL AND verified_by IS NULL)
    AND (confirmed_by IS NULL OR verified_by IS NULL)
);
ALTER TABLE resolutions ADD CONSTRAINT resolutions_confirmed_before_settling CHECK (
    state NOT IN ('confirmed', 'settling', 'settled') OR confirmed_at IS NOT NULL
);
