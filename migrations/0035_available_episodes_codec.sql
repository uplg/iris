-- Coarse video codec ("h264" / "hevc" / "av1" / "vp9" / "unknown")
-- parsed from the release title at scan time. Lets the grab path
-- follow the series' established codec ("the household watches this
-- show in x265") without re-fetching titles from the indexer.
-- NULL on legacy rows until the scheduler re-records them; a NULL
-- codec is treated as unknown (tolerated, never preferred).
ALTER TABLE available_episodes ADD COLUMN codec TEXT;
