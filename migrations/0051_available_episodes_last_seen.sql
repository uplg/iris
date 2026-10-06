-- When an indexer scan last returned the offer (`found_at` is when it first
-- did). The maintenance loop drops offers no scan has seen for a month: a
-- release pulled from its tracker otherwise stays a grab candidate forever.
ALTER TABLE available_episodes ADD COLUMN last_seen_at TIMESTAMP;
UPDATE available_episodes SET last_seen_at = found_at;
CREATE INDEX available_episodes_last_seen_idx ON available_episodes(last_seen_at);
