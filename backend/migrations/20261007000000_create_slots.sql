-- One row per held place in a match: the holder's own place (guest_name NULL)
-- or a named guest the holder brings. Leaving sets released_at; rows are kept
-- for attendance history (no-show marking comes later).
CREATE TABLE slots (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    match_id bigint NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
    team text NOT NULL CHECK (team IN ('a', 'b')),
    holder_user_id bigint NOT NULL REFERENCES users (id),
    -- Public (spec.md "Public visibility"): shown on the match page.
    guest_name text CHECK (char_length(guest_name) BETWEEN 1 AND 40),
    claimed_at timestamptz NOT NULL,
    released_at timestamptz CHECK (released_at >= claimed_at)
);

-- A user holds at most one own place per match. Capacity and the 2-guest limit
-- are enforced in the claiming transaction, under a lock on the match row.
CREATE UNIQUE INDEX slots_one_own_place_idx
    ON slots (match_id, holder_user_id)
    WHERE guest_name IS NULL AND released_at IS NULL;
CREATE INDEX slots_active_by_match_idx ON slots (match_id) WHERE released_at IS NULL;
CREATE INDEX slots_holder_idx ON slots (holder_user_id);
