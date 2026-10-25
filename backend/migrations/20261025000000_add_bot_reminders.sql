-- Reminders "Bot DaGhep" posts to a Zalo chat on a schedule
-- ("@Bot DaGhep nhắc mỗi thứ 5 20h: cuối tuần ai đá không?").
-- See docs/zalo-bot-plan.md, step 4.

-- A reminder read from a message, waiting until the same sender answers "ok"
-- in the same chat (30 minutes). A sender has at most one pending item per chat:
-- saving a reminder removes their pending match draft there, and the other way round.
CREATE TABLE bot_pending_reminders (
    zalo_user_id text NOT NULL CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Bit n is weekday n counted from Sunday = 0; 0 for a monthly reminder.
    weekdays smallint NOT NULL CHECK (weekdays BETWEEN 0 AND 127),
    day_of_month smallint CHECK (day_of_month BETWEEN 1 AND 28),
    time_of_day time NOT NULL,
    text text NOT NULL CHECK (char_length(text) BETWEEN 1 AND 200),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (zalo_user_id, chat_id),
    CHECK ((weekdays = 0) = (day_of_month IS NOT NULL))
);

CREATE INDEX bot_pending_reminders_expires_at_idx ON bot_pending_reminders (expires_at);

-- Saved reminders, at most 3 per chat. Times are Ho Chi Minh City time.
-- Deleted with the account that created them, or after 3 failed sends in a row.
CREATE TABLE bot_reminders (
    id bigserial PRIMARY KEY,
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    weekdays smallint NOT NULL CHECK (weekdays BETWEEN 0 AND 127),
    day_of_month smallint CHECK (day_of_month BETWEEN 1 AND 28),
    time_of_day time NOT NULL,
    text text NOT NULL CHECK (char_length(text) BETWEEN 1 AND 200),
    next_run_at timestamptz NOT NULL,
    failures smallint NOT NULL DEFAULT 0 CHECK (failures >= 0),
    created_at timestamptz NOT NULL,
    CHECK ((weekdays = 0) = (day_of_month IS NOT NULL))
);

CREATE INDEX bot_reminders_next_run_at_idx ON bot_reminders (next_run_at);
CREATE INDEX bot_reminders_chat_id_idx ON bot_reminders (chat_id, id);
