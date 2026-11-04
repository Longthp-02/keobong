-- Group games and follow-up messages (Long, 2026-11-04): a one-off poll for a
-- casual game ("tạo kèo bóng rổ hôm nay lakeview 7h") keeps its venue, and a
-- request that left something out waits 30 minutes for the sender's next
-- message, which is read together with it ("19h30").

-- The venue of a poll; weekly polls also have it on their series. A one-off
-- game with a venue shows on the home page (title, sport, venue, day, time and
-- yes-counts, no names) unless the request says "riêng tư" (Long, 2026-11-04).
ALTER TABLE bot_polls
    ADD COLUMN venue_id bigint REFERENCES venues (id),
    ADD COLUMN public boolean NOT NULL DEFAULT false;

CREATE INDEX bot_polls_public_idx ON bot_polls (closes_at) WHERE public;

-- The sender's last incomplete request in a chat: a match or a poll, its text
-- so far, and when it is forgotten. Replaced by the next one, deleted once the
-- request is complete, cancelled, or expired.
CREATE TABLE bot_partial_requests (
    zalo_user_id text NOT NULL CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    kind text NOT NULL CHECK (kind IN ('match', 'poll')),
    text text NOT NULL CHECK (char_length(text) BETWEEN 1 AND 2000),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (zalo_user_id, chat_id)
);

CREATE INDEX bot_partial_requests_expires_at_idx ON bot_partial_requests (expires_at);
