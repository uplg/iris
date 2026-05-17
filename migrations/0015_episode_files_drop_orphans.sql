-- Purge orphaned episode_files rows.
--
-- Removing a torrent soft-deletes its `torrents` row, drops the librqbit
-- handle and wipes its files — but historically left the matching
-- `episode_files` (collection, season, episode) → infohash rows behind.
-- The series / collection browse views read straight from `episode_files`,
-- so those dangling rows surfaced as "stale collection entities":
-- episodes pointing at an infohash whose torrent no longer exists, which
-- on click spin the player on "Reading media metadata…" forever.
--
-- The removal path now cascades (`episode_files::delete_for_infohash`) and
-- the read paths self-heal via an `EXISTS (… deleted_at IS NULL)` filter.
-- This one-off purge clears the pre-existing pile so the self-heal filter
-- isn't masking an ever-growing set of dead rows. Covers both soft-deleted
-- torrents and infohashes whose `torrents` row is gone entirely.
DELETE FROM episode_files
WHERE infohash NOT IN (
    SELECT infohash FROM torrents WHERE deleted_at IS NULL
);
