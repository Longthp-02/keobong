-- Host payout accounts and per-place payment status with a 30-minute hold.

-- Sensitive: shown only to players holding a place in this host's match.
CREATE TABLE payout_accounts (
    user_id bigint PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    bank_bin text NOT NULL CHECK (bank_bin ~ '^[0-9]{6}$'),
    account_number text NOT NULL CHECK (account_number ~ '^[0-9]{6,19}$'),
    account_name text NOT NULL CHECK (account_name ~ '^[A-Z ]{2,50}$'),
    updated_at timestamptz NOT NULL
);

-- Every place in a party (holder plus guests) carries the party's status:
--   awaiting_payment  held until hold_expires_at, then treated as released
--   payment_reported  the player says they transferred; waits for the host
--   confirmed         the host confirmed, or the match is free
-- Existing rows predate payments and count as confirmed.
ALTER TABLE slots
    ADD COLUMN payment_status text NOT NULL DEFAULT 'confirmed'
        CHECK (payment_status IN ('awaiting_payment', 'payment_reported', 'confirmed')),
    ADD COLUMN hold_expires_at timestamptz,
    ADD CONSTRAINT slots_hold_only_while_awaiting
        CHECK ((payment_status = 'awaiting_payment') = (hold_expires_at IS NOT NULL));
ALTER TABLE slots ALTER COLUMN payment_status DROP DEFAULT;
