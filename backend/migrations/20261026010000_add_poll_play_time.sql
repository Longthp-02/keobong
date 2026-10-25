-- When the players play, shown in the bot's messages and on the poll page
-- (Long, 2026-10-26). Polls made before this change have none.
ALTER TABLE bot_polls ADD COLUMN play_time time;
ALTER TABLE bot_pending_polls ADD COLUMN play_time time;
