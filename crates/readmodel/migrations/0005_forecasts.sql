-- Forecasts and forecaster scores (SPEC §7, docs/gates/G6-plan.md).

-- Every forecast a user records: a probability from 1 to 99 percent, timestamped by the server.
-- Append-only: a change is a new row, and the trigger below refuses edits and deletions, so a
-- forecast cannot be backdated or rewritten.
CREATE TABLE forecasts (
    id bigserial PRIMARY KEY,
    user_id bigint NOT NULL REFERENCES users (id),
    market bigint NOT NULL REFERENCES rulebooks (market),
    probability smallint NOT NULL CHECK (probability BETWEEN 1 AND 99),
    at timestamptz NOT NULL
);
CREATE INDEX forecasts_by_market ON forecasts (market, user_id, at);
CREATE INDEX forecasts_by_user ON forecasts (user_id, at DESC);

CREATE FUNCTION forecasts_are_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'forecasts are append-only';
END
$$;
CREATE TRIGGER forecasts_append_only BEFORE UPDATE OR DELETE ON forecasts
    FOR EACH ROW EXECUTE FUNCTION forecasts_are_append_only();

-- Once a minute, each open market's price (percent): the last trade, or before the first trade
-- the book midpoint (one side alone if the other is empty; 50 if both are). Skill is averaged
-- over these samples.
CREATE TABLE score_samples (
    market bigint NOT NULL REFERENCES rulebooks (market),
    at timestamptz NOT NULL,
    price smallint NOT NULL CHECK (price BETWEEN 1 AND 99),
    PRIMARY KEY (market, at)
);

-- Each user's score on each market that resolved YES or NO, in units of 1/10,000.
CREATE TABLE scores (
    market bigint NOT NULL REFERENCES rulebooks (market),
    user_id bigint NOT NULL REFERENCES users (id),
    outcome text NOT NULL CHECK (outcome IN ('yes', 'no')),
    skill integer NOT NULL CHECK (skill BETWEEN -10000 AND 10000),
    user_brier integer NOT NULL CHECK (user_brier BETWEEN 0 AND 10000),
    market_brier integer NOT NULL CHECK (market_brier BETWEEN 0 AND 10000),
    samples integer NOT NULL CHECK (samples > 0),
    forecasts integer NOT NULL CHECK (forecasts > 0),
    PRIMARY KEY (market, user_id)
);
CREATE INDEX scores_by_user ON scores (user_id);

-- Markets already scored (one row per market, written in the same statement as its scores).
CREATE TABLE scored_markets (
    market bigint PRIMARY KEY REFERENCES rulebooks (market),
    outcome text NOT NULL CHECK (outcome IN ('yes', 'no')),
    samples integer NOT NULL,
    scored_at timestamptz NOT NULL
);
