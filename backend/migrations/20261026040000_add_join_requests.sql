-- Join requests for public weekly groups (docs/public-groups-plan.md, step 2).
-- Approved by Long 2026-10-26: a player outside the Zalo group asks to join
-- from the home page; the organiser approves or declines through the bot.

-- A short id the home page links to (/g/<id>). Public, not a secret. Existing
-- rows get their own value: a volatile default is evaluated per row.
ALTER TABLE bot_poll_series
    ADD COLUMN public_id text NOT NULL UNIQUE
        DEFAULT substr(md5(random()::text || clock_timestamp()::text), 1, 12)
        CHECK (public_id ~ '^[a-f0-9]{12}$'),
    -- The group's Zalo invite link, set by the organiser and shown only to members.
    ADD COLUMN zalo_group_link text
        CHECK (zalo_group_link ~ '^https://zalo\.me/g/[A-Za-z0-9]{1,64}$');

-- One request per player and group. A declined player may ask again after
-- 7 days, which turns the same row back into a pending request.
CREATE TABLE bot_join_requests (
    id bigserial PRIMARY KEY,
    series_id bigint NOT NULL REFERENCES bot_poll_series (id) ON DELETE CASCADE,
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status text NOT NULL CHECK (status IN ('pending', 'approved', 'rejected')),
    requested_at timestamptz NOT NULL,
    decided_at timestamptz,
    UNIQUE (series_id, user_id),
    CHECK ((status = 'pending') = (decided_at IS NULL))
);

-- Limits: pending requests per player, and per organiser through their series.
CREATE INDEX bot_join_requests_user_pending_idx ON bot_join_requests (user_id)
    WHERE status = 'pending';
CREATE INDEX bot_join_requests_series_pending_idx ON bot_join_requests (series_id)
    WHERE status = 'pending';
