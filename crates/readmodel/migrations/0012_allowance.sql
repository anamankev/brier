-- The daily allowance (G10 U2): one claim per user per UTC day, by the gateway's clock. The
-- grant itself is a journaled command; this table only stops a second claim on the same day.
CREATE TABLE allowance_claims (
    user_id bigint NOT NULL REFERENCES users (id),
    day date NOT NULL,
    seq bigint,                     -- the journaled grant, once committed
    PRIMARY KEY (user_id, day)
);
