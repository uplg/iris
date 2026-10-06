-- 1. tmdb_resolve_cache keyed `(cleaned_name, kind_hint)` with a NULL-able
--    kind: SQLite treats NULLs as distinct in a key, so every unhinted put
--    added a row instead of replacing it. Keep the newest row per name,
--    then store "no hint" as '' so the upsert's conflict target matches.
DELETE FROM tmdb_resolve_cache
WHERE rowid NOT IN (
    SELECT rowid FROM (
        SELECT rowid,
               ROW_NUMBER() OVER (
                   PARTITION BY cleaned_name, COALESCE(kind_hint, '')
                   ORDER BY fetched_at DESC
               ) AS rn
        FROM tmdb_resolve_cache
    )
    WHERE rn = 1
);
UPDATE tmdb_resolve_cache SET kind_hint = '' WHERE kind_hint IS NULL;

-- 2. playback_progress only had its primary key and a partial index on
--    unfinished rows. History (all rows, newest first), the admin's
--    cross-user activity feed, next-up (completed rows) and the join from
--    a torrent file to its progress all scanned the table.
CREATE INDEX playback_progress_user_watched
    ON playback_progress(user_id, last_watched_at DESC);
CREATE INDEX playback_progress_watched
    ON playback_progress(last_watched_at DESC);
CREATE INDEX playback_progress_file
    ON playback_progress(infohash, file_idx);
