-- Collections-driven scheduler + per-collection "new episodes" badge.
--
-- The follow concept is being retired: any TV collection (one that
-- has at least one ingested episode) is now what the indexer scanner
-- iterates over. Two columns power that:
--
--   * last_indexer_scan_at — pacing cursor for the periodic scan
--     (mirror of series_follows.last_checked_at). NULL means
--     "never scanned"; the scheduler picks new collections up on
--     the next tick and stops re-querying them inside the cooldown
--     window thereafter.
--
--   * last_visited_at — touched whenever a user opens the
--     collection detail page. Drives the home-page Watchlist badge
--     ("3 new") by counting available_episodes whose found_at is
--     greater than this stamp and which aren't already on disk.
--     Single-tenant household so storing it on the collection
--     itself (not per-user) is correct; revisit if Iris ever
--     becomes multi-household.

ALTER TABLE collections ADD COLUMN last_indexer_scan_at TIMESTAMP;
ALTER TABLE collections ADD COLUMN last_visited_at      TIMESTAMP;

-- The scheduler picks the oldest-scanned collections first. NULLs sort
-- first in SQLite, which is exactly what we want — freshly-ingested
-- TV collections get their initial scan before older ones recycle.
CREATE INDEX IF NOT EXISTS collections_scheduler_idx
    ON collections(last_indexer_scan_at)
    WHERE kind = 'tv';
