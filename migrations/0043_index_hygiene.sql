-- Index hygiene. Drops only indexes another index already covers (or that
-- no query uses), and adds the lookups that full-scanned. No data change.

-- `(normalized_name, season, episode)` is a prefix of the unique
-- `available_episodes_dedup_idx`; every scheduler upsert paid for both.
DROP INDEX IF EXISTS available_episodes_lookup_idx;
-- Duplicates of the UNIQUE constraints' own autoindexes.
DROP INDEX IF EXISTS users_email_idx;
DROP INDEX IF EXISTS invitations_token_hash_idx;
-- Served the retired follows scheduler; nothing filters or orders on
-- `last_checked_at` any more.
DROP INDEX IF EXISTS series_follows_scheduler_idx;

-- Catalogue preview / grab: `catalog::download_url_for`.
CREATE INDEX IF NOT EXISTS catalog_items_provider_external_idx
    ON catalog_items(provider_id, external_id);
-- Release page: `torrents::find_live_by_source`.
CREATE INDEX IF NOT EXISTS torrents_source_idx
    ON torrents(source_provider, source_external_id);
-- Foreign-key children whose parent column is the second primary-key
-- column: every catalogue prune or collection merge/delete scanned them.
CREATE INDEX IF NOT EXISTS reco_feedback_catalog_idx ON reco_feedback(catalog_id);
CREATE INDEX IF NOT EXISTS cw_dismissed_collection_idx ON cw_dismissed(collection_id);
CREATE INDEX IF NOT EXISTS ghost_dismissed_collection_idx ON ghost_dismissed(collection_id);
CREATE INDEX IF NOT EXISTS collection_playback_preferences_collection_idx
    ON collection_playback_preferences(collection_id);
