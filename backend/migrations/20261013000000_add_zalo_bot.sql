-- Zalo bot ("Bot DaGhep"): organisers create matches from a Zalo chat.
-- See docs/zalo-bot-plan.md (approved by Long 2026-10-12).

-- A Zalo account linked to a Đá Ghép account, so the bot knows who the host is.
-- One Zalo account per Đá Ghép account and the other way round.
CREATE TABLE zalo_links (
    zalo_user_id text PRIMARY KEY CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL UNIQUE REFERENCES users (id) ON DELETE CASCADE,
    linked_at timestamptz NOT NULL
);

-- One-time codes the bot sends privately to start linking. Only a SHA-256 hash
-- is stored; a code works once and expires after 15 minutes.
CREATE TABLE zalo_link_codes (
    code_hash bytea PRIMARY KEY CHECK (octet_length(code_hash) = 32),
    zalo_user_id text NOT NULL CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    expires_at timestamptz NOT NULL
);

CREATE INDEX zalo_link_codes_expires_at_idx ON zalo_link_codes (expires_at);

-- A match the bot read from a message and is waiting to create until the same
-- sender answers "ok" in the same chat (30 minutes). One per sender and chat.
CREATE TABLE bot_drafts (
    zalo_user_id text NOT NULL CHECK (char_length(zalo_user_id) BETWEEN 1 AND 64),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 64),
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    venue_slug text NOT NULL,
    starts_at timestamptz NOT NULL,
    ends_at timestamptz NOT NULL,
    format text NOT NULL,
    match_type text NOT NULL,
    level_min_tenths smallint NOT NULL,
    level_max_tenths smallint NOT NULL,
    slot_count smallint NOT NULL,
    total_fee_vnd bigint NOT NULL,
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (zalo_user_id, chat_id)
);

CREATE INDEX bot_drafts_expires_at_idx ON bot_drafts (expires_at);

-- Messages the bot answered: Zalo may deliver a message more than once, and each
-- sender gets at most 20 answers per hour. No message text is stored. Rows
-- older than two days are deleted.
CREATE TABLE bot_updates (
    message_id text PRIMARY KEY CHECK (char_length(message_id) BETWEEN 1 AND 128),
    sender_id text NOT NULL CHECK (char_length(sender_id) BETWEEN 1 AND 64),
    received_at timestamptz NOT NULL
);

CREATE INDEX bot_updates_received_at_idx ON bot_updates (received_at);
CREATE INDEX bot_updates_sender_received_at_idx ON bot_updates (sender_id, received_at);
