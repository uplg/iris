-- Speed up the personalization / reco joins in `catalog.rs`, which map a
-- user's plays/grabs to catalog_items via (tmdb_id, kind) across the WHOLE
-- table (anime included). The existing index on those columns
-- (`catalog_items_tmdb_kind_idx`, migration 0020) is PARTIAL —
-- `WHERE tmdb_id IS NOT NULL AND is_anime = 0` — so SQLite can't use it for a
-- lookup that doesn't constrain `is_anime`, and falls back to a full SCAN of
-- catalog_items per joined row (observed ~1.1 s for ~117 plays, tripping the
-- slow-statement alert). A plain covering index turns each join into a point
-- seek (`SEARCH … USING INDEX (tmdb_id=? AND kind=?)`, verified with
-- EXPLAIN QUERY PLAN).
CREATE INDEX IF NOT EXISTS catalog_items_tmdb_kind_lookup
    ON catalog_items(tmdb_id, kind);
