-- Public groups (docs/public-groups-plan.md, Long 2026-10-26): weekly poll
-- series can be shown on the home page with their sport, format and venue.

-- Venues added by organisers from a Google Maps link wait for an admin.
-- They are never offered for football matches (`active` stays false); the
-- launch venues stay as they are.
ALTER TABLE venues
    ADD COLUMN sports text[] NOT NULL DEFAULT '{football}'
        CHECK (sports <@ ARRAY['football', 'basketball']::text[] AND cardinality(sports) >= 1),
    ADD COLUMN status text NOT NULL DEFAULT 'approved'
        CHECK (status IN ('pending', 'approved', 'rejected')),
    ADD COLUMN submitted_by bigint REFERENCES users (id) ON DELETE SET NULL,
    ADD COLUMN maps_url text CHECK (char_length(maps_url) <= 500);

-- Names are unique among approved venues only, so a pending or rejected
-- proposal never blocks another group's proposal or a later launch venue.
ALTER TABLE venues DROP CONSTRAINT venues_name_key;
CREATE UNIQUE INDEX venues_approved_name_idx ON venues (lower(name)) WHERE status = 'approved';

-- What a weekly group plays and where, and whether it is shown publicly.
ALTER TABLE bot_poll_series
    ADD COLUMN sport text CHECK (sport IN ('football', 'basketball')),
    ADD COLUMN game_format text
        CHECK (game_format IN ('3x3', '5x5', 'five_a_side', 'seven_a_side', 'eleven_a_side')),
    ADD COLUMN venue_id bigint REFERENCES venues (id),
    ADD COLUMN public boolean NOT NULL DEFAULT false;

ALTER TABLE bot_pending_polls
    ADD COLUMN sport text CHECK (sport IN ('football', 'basketball')),
    ADD COLUMN game_format text
        CHECK (game_format IN ('3x3', '5x5', 'five_a_side', 'seven_a_side', 'eleven_a_side')),
    ADD COLUMN venue_id bigint REFERENCES venues (id),
    ADD COLUMN public boolean NOT NULL DEFAULT false;

-- Which series a poll belongs to, for the group's counts this week.
ALTER TABLE bot_polls
    ADD COLUMN series_id bigint REFERENCES bot_poll_series (id) ON DELETE SET NULL;

CREATE INDEX bot_polls_series_id_idx ON bot_polls (series_id, closes_at);
CREATE INDEX bot_poll_series_public_idx ON bot_poll_series (id) WHERE public;
