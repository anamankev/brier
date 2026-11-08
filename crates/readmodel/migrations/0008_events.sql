-- Events (G9; docs/rulebook.md, schema 2): markets grouped as the outcomes of one question.
-- Every market of an event shares its observation, source series, and close time; an
-- exclusive event's markets have YES ranges that do not overlap (checked by the gateway).
CREATE TABLE events (
    slug text PRIMARY KEY CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 80),
    title text NOT NULL,
    category text NOT NULL,
    tags text[] NOT NULL,
    series text,
    exclusive boolean NOT NULL,
    provider text NOT NULL,
    source_series text NOT NULL,
    observation text NOT NULL,
    close_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX events_by_series ON events (series, observation) WHERE series IS NOT NULL;
CREATE INDEX events_by_category ON events (category, close_at);

ALTER TABLE rulebooks ADD COLUMN event_slug text REFERENCES events (slug);
ALTER TABLE rulebooks ADD COLUMN outcome text;
ALTER TABLE rulebooks ADD COLUMN review text NOT NULL DEFAULT 'manual' CHECK (review IN ('manual', 'automatic'));
ALTER TABLE rulebooks ADD CONSTRAINT rulebooks_event_has_outcome CHECK ((event_slug IS NULL) = (outcome IS NULL));
CREATE UNIQUE INDEX rulebooks_one_outcome_label ON rulebooks (event_slug, outcome) WHERE event_slug IS NOT NULL;

-- Cumulative contracts traded in the market at each minute sample, for 24-hour volume.
ALTER TABLE score_samples ADD COLUMN volume bigint CHECK (volume >= 0);
