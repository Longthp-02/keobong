-- Weekly attendance polls (Long, 2026-10-26): "@Bot DaGhep tạo bảng điểm danh T7,
-- CN hằng tuần, chơi lúc 18h". Every Monday at the nudge time the bot posts a new
-- poll for that week's chosen weekdays, until someone stops it ("tắt bảng").

ALTER TABLE bot_pending_polls ADD COLUMN weekly boolean NOT NULL DEFAULT false;

-- At most 3 per chat. Deleted with the account that created it.
CREATE TABLE bot_poll_series (
    id bigserial PRIMARY KEY,
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    -- Bit n is weekday n counted from Sunday = 0.
    weekdays smallint NOT NULL CHECK (weekdays BETWEEN 1 AND 127),
    play_time time,
    nudge_time time NOT NULL,
    next_post_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL
);

CREATE INDEX bot_poll_series_next_post_at_idx ON bot_poll_series (next_post_at);
CREATE INDEX bot_poll_series_chat_id_idx ON bot_poll_series (chat_id, id);
