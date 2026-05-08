-- Capture the TMDB id at ingest time. torr9 (and most providers) returns it
-- on the search hit, but until now we dropped it on the floor — which meant
-- continue-watching cards and library cards had no way to fetch posters /
-- backdrops from TMDB without a slow fuzzy title-year lookup.
--
-- Optional column: not every provider/result has tmdb_id, and old torrents
-- ingested before this migration won't have one either. Frontend code must
-- tolerate NULL.

ALTER TABLE torrents ADD COLUMN tmdb_id INTEGER;
CREATE INDEX torrents_tmdb_id_idx ON torrents(tmdb_id) WHERE tmdb_id IS NOT NULL;
