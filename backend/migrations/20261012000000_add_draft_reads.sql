-- One row per request to read an organiser's message with the AI reader, kept
-- only to enforce the daily limits (10 per user, 200 in total, rolling 24 hours).
-- The message itself is never stored. Rows older than 24 hours are deleted by
-- the next read.
CREATE TABLE draft_reads (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL
);

CREATE INDEX draft_reads_user_created_at_idx ON draft_reads (user_id, created_at);
CREATE INDEX draft_reads_created_at_idx ON draft_reads (created_at);
