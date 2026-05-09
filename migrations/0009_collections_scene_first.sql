-- Make SCENE-parsed filename info the source of truth for collection
-- identity AND for episode-file mapping. TMDB id stays on the table as
-- enrichment metadata (poster / synopsis lookups), but no longer
-- governs grouping.
--
-- Existing collections were grouped TMDB-first, which produced
-- "totalement pétées" rows when the indexer mis-tagged a torrent
-- (wrong TMDB id → wrong title shown, sometimes a completely
-- different show / movie). Wipe the slate and let the backfill job
-- rebuild from SCENE truth on next boot.

-- 1. Multiple collections may now legitimately share a tmdb_id (when
--    the indexer mis-tags). Drop the partial unique index that was
--    enforcing one-collection-per-tmdb. Replace with a plain lookup
--    index — the follow → collection join still needs it.
DROP INDEX IF EXISTS collections_tmdb_idx;
CREATE INDEX collections_tmdb_lookup_idx
    ON collections(tmdb_id) WHERE tmdb_id IS NOT NULL;

-- 2. Reset every torrent's back-reference; backfill repopulates.
UPDATE torrents SET collection_id = NULL;

-- 3. Wipe collections themselves.
DELETE FROM collections;

-- 4. Re-key episode_files on collection_id (was tmdb_id). The old
--    scheme contaminated Watchlists when an unrelated torrent got
--    tagged with the right show's tmdb_id by mistake — episodes
--    showed up "downloaded" for shows that hadn't been touched.
--    SQLite's column-level migration is awkward; the table content
--    is being wiped anyway, so DROP + CREATE is cleanest.
DROP TABLE episode_files;

CREATE TABLE episode_files (
    id              BLOB    PRIMARY KEY NOT NULL,
    collection_id   BLOB    NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    season          INTEGER NOT NULL,
    episode         INTEGER NOT NULL,
    infohash        TEXT    NOT NULL,
    file_idx        INTEGER NOT NULL,
    -- 'scene_parse'  — regex on the filename (Show.Name.S01E02.…)
    -- 'tmdb_match'   — user-driven on-demand grab; we know S/E because
    --                  the grab params said so, not from a filename hit
    -- 'manual'       — user assigned via admin UI (future)
    derived_from    TEXT    NOT NULL CHECK (derived_from IN ('tmdb_match', 'scene_parse', 'manual')),
    created_at      TIMESTAMP NOT NULL,
    UNIQUE (infohash, file_idx)
);

-- Read pattern: "give me everything we have for collection X, season N".
CREATE INDEX episode_files_lookup_idx
    ON episode_files(collection_id, season, episode);
