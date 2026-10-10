-- A host can cancel a match before kickoff; the match stays visible as cancelled.
ALTER TABLE matches ADD COLUMN cancelled_at timestamptz;
