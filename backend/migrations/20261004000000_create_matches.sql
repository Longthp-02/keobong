-- Matches: one scheduled pickup game. Public reads go through `share_id`;
-- the numeric `id` is internal and must never appear in URLs or responses.
CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE matches (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    share_id         TEXT        NOT NULL UNIQUE
                                 CHECK (share_id ~ '^[A-Za-z0-9_-]{8,16}$'),
    venue_name       TEXT        NOT NULL
                                 CHECK (char_length(venue_name) BETWEEN 1 AND 120),
    location         geography(Point, 4326),
    starts_at        TIMESTAMPTZ NOT NULL,
    ends_at          TIMESTAMPTZ NOT NULL,
    format           TEXT        NOT NULL
                                 CHECK (format IN ('five_a_side', 'seven_a_side', 'eleven_a_side')),
    match_type       TEXT        NOT NULL
                                 CHECK (match_type IN ('casual', 'competitive', 'beginner_friendly')),
    -- Levels are stored in tenths (1.0 -> 10, 5.0 -> 50) to avoid floats; steps of 0.5.
    level_min_tenths SMALLINT    NOT NULL
                                 CHECK (level_min_tenths BETWEEN 10 AND 50 AND level_min_tenths % 5 = 0),
    level_max_tenths SMALLINT    NOT NULL
                                 CHECK (level_max_tenths BETWEEN 10 AND 50 AND level_max_tenths % 5 = 0),
    -- Money is integer VND. Whether a free (zero-fee) match is allowed: TODO: verify.
    total_fee_vnd    BIGINT      NOT NULL CHECK (total_fee_vnd >= 0),
    -- Upper bound is a sanity guard, not a product rule (capacity per format is TODO: verify).
    slot_count       SMALLINT    NOT NULL CHECK (slot_count BETWEEN 2 AND 30),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (ends_at > starts_at),
    CHECK (level_max_tenths >= level_min_tenths)
);

CREATE INDEX matches_location_gix ON matches USING GIST (location);
CREATE INDEX matches_starts_at_idx ON matches (starts_at);
