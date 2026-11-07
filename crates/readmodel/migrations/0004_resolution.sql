-- Rulebooks and the resolution workflow (docs/gates/G5-plan.md, docs/resolution.md).

-- Read model: what the journal itself says about each market's rules and settlement. Projected
-- from `market_created` and `settled` events like every other read model, so it can be rebuilt
-- from the journal alone. The gateway compares the rulebooks and resolutions below with it.
CREATE TABLE market_journal (
    market bigint PRIMARY KEY CHECK (market BETWEEN 0 AND 4294967295),
    listed_seq bigint NOT NULL,
    rules_hash bytea CHECK (length(rules_hash) = 32),       -- NULL only for markets listed before G5
    settled_seq bigint,
    outcome text CHECK (outcome IN ('yes', 'no', 'void')),
    evidence_hash bytea CHECK (length(evidence_hash) = 32),
    CHECK ((settled_seq IS NULL) = (outcome IS NULL))
);

-- A third role: a resolver may propose outcomes and nothing else.
ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('trader', 'admin', 'resolver'));

-- Commands the gateway itself sends (the scheduler and the settler) are audited as `system`.
ALTER TABLE audit_log ADD COLUMN actor text NOT NULL DEFAULT 'user' CHECK (actor IN ('user', 'system'));
ALTER TABLE audit_log ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_user_iff_actor_user CHECK ((actor = 'user') = (user_id IS NOT NULL));

-- The rulebook each market was listed under: its canonical JSON (RFC 8785) and SHA-256, which
-- the market's `create_market` command carries into the journal.
CREATE TABLE rulebooks (
    market bigint PRIMARY KEY CHECK (market BETWEEN 1 AND 4294967295),
    canonical text NOT NULL,
    rules_hash bytea NOT NULL UNIQUE CHECK (length(rules_hash) = 32),
    open_at timestamptz NOT NULL,
    close_at timestamptz NOT NULL,
    release_at timestamptz NOT NULL,
    deadline_at timestamptz NOT NULL,
    listed_by bigint NOT NULL REFERENCES users (id),
    listed_at timestamptz NOT NULL DEFAULT now(),
    CHECK (open_at < close_at AND close_at < release_at AND release_at < deadline_at)
);

-- Evidence records, keyed by the SHA-256 of their canonical JSON. The raw source response a
-- record names is kept by the resolver under var/evidence/<sha256>.
CREATE TABLE evidence (
    evidence_hash bytea PRIMARY KEY CHECK (length(evidence_hash) = 32),
    market bigint NOT NULL REFERENCES rulebooks (market),
    canonical text NOT NULL,
    submitted_by bigint NOT NULL REFERENCES users (id),
    submitted_at timestamptz NOT NULL DEFAULT now()
);

-- One row per proposed outcome. At most one per market is live (any state but withdrawn); a
-- withdrawn proposal stays as history. `settling` means the settler has claimed it: from then
-- on it cannot be withdrawn or disputed, and after a crash the settler finishes it.
CREATE TABLE resolutions (
    id bigserial PRIMARY KEY,
    market bigint NOT NULL REFERENCES rulebooks (market),
    outcome text NOT NULL CHECK (outcome IN ('yes', 'no', 'void')),
    void_condition text CHECK (void_condition IN ('no_value_by_deadline', 'source_discontinued')),
    evidence_hash bytea NOT NULL REFERENCES evidence (evidence_hash),
    state text NOT NULL CHECK (state IN ('proposed', 'confirmed', 'settling', 'settled', 'withdrawn')),
    proposed_by bigint NOT NULL REFERENCES users (id),
    proposed_at timestamptz NOT NULL,
    proposal_reason text NOT NULL CHECK (length(proposal_reason) BETWEEN 3 AND 500),
    confirmed_by bigint REFERENCES users (id),
    confirmed_at timestamptz,
    dispute_until timestamptz,
    withdrawn_by bigint REFERENCES users (id),               -- NULL with state 'withdrawn' means a dispute was upheld
    withdrawn_at timestamptz,
    settled_seq bigint,
    CHECK ((outcome = 'void') = (void_condition IS NOT NULL)),
    CHECK (confirmed_by IS NULL OR confirmed_by <> proposed_by),   -- four eyes
    CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL) AND (confirmed_at IS NULL) = (dispute_until IS NULL)),
    CHECK (state NOT IN ('confirmed', 'settling', 'settled') OR confirmed_by IS NOT NULL),
    CHECK ((state = 'settled') = (settled_seq IS NOT NULL)),
    CHECK ((state = 'withdrawn') = (withdrawn_at IS NOT NULL))
);
CREATE UNIQUE INDEX resolutions_one_live ON resolutions (market) WHERE state <> 'withdrawn';

-- Disputes against a confirmed resolution, one per position holder per resolution.
CREATE TABLE disputes (
    id bigserial PRIMARY KEY,
    resolution bigint NOT NULL REFERENCES resolutions (id),
    user_id bigint NOT NULL REFERENCES users (id),
    reason text NOT NULL CHECK (length(reason) BETWEEN 3 AND 500),
    filed_at timestamptz NOT NULL,
    state text NOT NULL CHECK (state IN ('open', 'upheld', 'rejected', 'moot')),
    ruled_by bigint REFERENCES users (id),
    ruled_at timestamptz,
    ruling_reason text CHECK (length(ruling_reason) BETWEEN 3 AND 500),
    UNIQUE (resolution, user_id),
    CHECK ((state = 'open') = (ruled_at IS NULL)),
    CHECK ((state IN ('upheld', 'rejected')) = (ruled_by IS NOT NULL))
);
CREATE INDEX disputes_open ON disputes (resolution) WHERE state = 'open';
