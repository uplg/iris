-- Persist the lifetime upload counter per torrent. librqbit's `uploaded_bytes`
-- is a session counter that resets on restart and disappears when a torrent
-- is removed from the engine (GC eviction); we want a "since the beginning"
-- total that survives both, so the seedbox can show how much it has actually
-- given back to the swarm.
--
-- `uploaded_bytes_total` is the running lifetime counter.
-- `uploaded_bytes_session_seen` is the last librqbit session value we saw,
-- used to compute deltas (see `iris_db::torrents::reconcile_uploaded`).
ALTER TABLE torrents ADD COLUMN uploaded_bytes_total INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrents ADD COLUMN uploaded_bytes_session_seen INTEGER NOT NULL DEFAULT 0;
