-- Attendance polls "Bot DaGhep" posts to a Zalo chat. Players answer yes or no
-- for each day on the web, signed in; once a day the bot posts the counts and
-- who has not answered. See docs/zalo-bot-plan.md, step 5.

-- A poll read from a message, waiting until the same sender answers "ok" in the
-- same chat (30 minutes). Saving it removes the sender's other pending items there.
CREATE TABLE bot_pending_polls (
    zalo_user_id text NOT NULL CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    days date[] NOT NULL CHECK (cardinality(days) BETWEEN 1 AND 7),
    nudge_time time NOT NULL,
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (zalo_user_id, chat_id)
);

CREATE INDEX bot_pending_polls_expires_at_idx ON bot_pending_polls (expires_at);

-- Saved polls, at most 3 open per chat. Days and times are Ho Chi Minh City time.
CREATE TABLE bot_polls (
    id bigserial PRIMARY KEY,
    share_id text NOT NULL UNIQUE CHECK (share_id ~ '^[A-Za-z0-9_-]{8,16}$'),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    days date[] NOT NULL CHECK (cardinality(days) BETWEEN 1 AND 7),
    nudge_time time NOT NULL,
    -- NULL once the last nudge is posted.
    next_nudge_at timestamptz,
    closes_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL
);

CREATE INDEX bot_polls_next_nudge_at_idx ON bot_polls (next_nudge_at) WHERE next_nudge_at IS NOT NULL;
CREATE INDEX bot_polls_chat_id_idx ON bot_polls (chat_id, closes_at);

-- One answer per player and day. Names are shown to anyone with the poll's link,
-- like the players of a match.
CREATE TABLE bot_poll_answers (
    poll_id bigint NOT NULL REFERENCES bot_polls (id) ON DELETE CASCADE,
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    day date NOT NULL,
    attending boolean NOT NULL,
    answered_at timestamptz NOT NULL,
    PRIMARY KEY (poll_id, user_id, day)
);

-- Linked users seen tagging the bot in a chat: with the people who answered the
-- chat's polls, they are who a nudge lists when they have not answered.
CREATE TABLE bot_chat_members (
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    last_seen_at timestamptz NOT NULL,
    PRIMARY KEY (chat_id, user_id)
);
