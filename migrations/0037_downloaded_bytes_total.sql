-- Lifetime download counter, mirroring uploaded_bytes_total: the global
-- "Seeded all-time" ratio divided lifetime upload by CURRENT disk usage,
-- overstating it as soon as the GC evicted anything, and the per-torrent
-- ratio divided lifetime upload by the live session's progress (a regrab
-- inherits its previous life's upload → instant bogus ratios).
--
-- Unlike upload, librqbit's progress is an absolute on-disk state rather
-- than a session counter, so "max ever seen" (reconciled by the 30 s
-- seed-stats tick) approximates bytes fetched. A re-download after GC
-- eviction is deliberately NOT double-counted — delta-tracking would
-- phantom-count the recheck climb after every restart instead.
ALTER TABLE torrents ADD COLUMN downloaded_bytes_total INTEGER NOT NULL DEFAULT 0;

-- Backfill: a torrent that reached completion downloaded its full size,
-- including soft-deleted (evicted) rows, which keep their finished_at.
-- Unfinished live rows self-correct on the next reconcile tick;
-- unfinished deleted rows are unknowable and stay at 0.
UPDATE torrents
SET downloaded_bytes_total = total_size_bytes
WHERE finished_at IS NOT NULL;
