-- Venues: the pitches hosts can choose from (a fixed list at launch, confirmed by
-- Long 2026-10-10). Public reads use `slug`; the numeric id stays internal.
CREATE TABLE venues (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    slug       TEXT        NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
    name       TEXT        NOT NULL UNIQUE CHECK (char_length(name) BETWEEN 1 AND 120),
    address    TEXT        NOT NULL CHECK (char_length(address) BETWEEN 1 AND 200),
    location   geography(Point, 4326) NOT NULL,
    -- Inactive venues stay on old matches but cannot be chosen for new ones.
    active     BOOLEAN     NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Matches created before venues existed keep only their venue name.
-- New matches copy the venue's location into matches.location for distance queries.
ALTER TABLE matches ADD COLUMN venue_id BIGINT REFERENCES venues (id);
CREATE INDEX matches_venue_id_idx ON matches (venue_id);
-- The match list is served by the existing matches_starts_at_idx (checked with EXPLAIN).
