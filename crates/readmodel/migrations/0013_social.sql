-- The social layer (G10 U3).
--
-- When the projector first wrote each command's fills, for "2 minutes ago" on activity feeds.
-- The journal carries no wall-clock time, so this table is the one read model that is not a
-- pure function of the journal: while the projector keeps up it is within a second of the
-- trade; a rebuild into an empty database stamps the rebuild time, and a rebuild that keeps
-- this table keeps the original times. Every other read model stays rebuildable.
CREATE TABLE fill_times (
    seq bigint PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now()
);

-- How far each user has read their notifications.
ALTER TABLE users ADD COLUMN seen_seq bigint NOT NULL DEFAULT 0 CHECK (seen_seq >= 0);

CREATE INDEX fills_by_market_maker ON fills (market, maker_account);
CREATE INDEX fills_by_market_taker ON fills (market, taker_account);
