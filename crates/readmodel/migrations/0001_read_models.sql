-- Read models projected from the journal (docs/gates/G4-plan.md). Everything here can be
-- rebuilt from the journal; the projector writes each batch and its cursor in one transaction.

CREATE TABLE projector_cursor (
    id smallint PRIMARY KEY CHECK (id = 1),
    last_seq bigint NOT NULL CHECK (last_seq >= 0)
);
INSERT INTO projector_cursor (id, last_seq) VALUES (1, 0);

-- One row per execution.
CREATE TABLE fills (
    seq bigint NOT NULL,
    ordinal integer NOT NULL,          -- position of the trade within its command's events
    market bigint NOT NULL,
    maker bigint NOT NULL,
    taker bigint NOT NULL,
    maker_account bigint NOT NULL,
    taker_account bigint NOT NULL,
    taker_side text NOT NULL CHECK (taker_side IN ('buy', 'sell')),
    price smallint NOT NULL CHECK (price BETWEEN 1 AND 99),
    qty integer NOT NULL CHECK (qty > 0),
    PRIMARY KEY (seq, ordinal)
);
CREATE INDEX fills_by_maker_account ON fills (maker_account, seq);
CREATE INDEX fills_by_taker_account ON fills (taker_account, seq);

-- Every order-related event, for order history.
CREATE TABLE order_events (
    seq bigint NOT NULL,
    ordinal integer NOT NULL,
    order_id bigint,                   -- NULL for a rejected place (no order was created)
    account bigint NOT NULL,
    market bigint NOT NULL,
    kind text NOT NULL CHECK (kind IN ('accepted', 'rested', 'amended', 'cancelled', 'duplicate', 'rejected')),
    remaining integer,
    reason text,
    PRIMARY KEY (seq, ordinal)
);
CREATE INDEX order_events_by_account ON order_events (account, seq);
CREATE INDEX order_events_by_order ON order_events (order_id, seq);
