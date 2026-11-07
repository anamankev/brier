-- Public trade history per market (GET /v1/markets/{id}/trades), newest first.
CREATE INDEX fills_by_market ON fills (market, seq DESC, ordinal DESC);
