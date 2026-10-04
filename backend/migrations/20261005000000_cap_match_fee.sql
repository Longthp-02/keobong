-- Defense in depth for the application guard (MAX_TOTAL_FEE_VND = 100 million VND,
-- confirmed by Long 2026-10-04); keeps fees in JavaScript's safe-integer range.
ALTER TABLE matches
    ADD CONSTRAINT matches_total_fee_vnd_max CHECK (total_fee_vnd <= 100000000);
