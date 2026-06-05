-- Rolling-window release facts on catalog_items (tracker-first discovery).
--
-- The reco catalogue flips from "TMDB/AniList metadata, availability unknown"
-- to a tracker RSS rolling window: the freshness scheduler polls each
-- provider's latest releases, correlates each to TMDB/AniList, and records the
-- best grabbable release's facts here so a click can ingest without a fresh
-- search. TMDB/AniList now serve correlation + recommendations only — they no
-- longer seed the catalogue on their own.
--
-- All new columns are nullable (additive, backward-compatible): legacy rows and
-- lazy recommendation candidates (TMDB-only, resolved on click) carry none.
--   * `released_at`  — the tracker upload time; basis for the sliding window
--                      ordering ("freshest first") and the window GC.
--   * `seeders`      — health of the recorded release; the dead-torrent guard
--                      never stores a 0-seeder release, and re-checks at grab.
--   * provider_id / external_id / download_url / infohash / language —
--                      enough to ingest the exact release directly.

ALTER TABLE catalog_items ADD COLUMN provider_id  TEXT;
ALTER TABLE catalog_items ADD COLUMN external_id  TEXT;
ALTER TABLE catalog_items ADD COLUMN download_url TEXT;
ALTER TABLE catalog_items ADD COLUMN infohash     TEXT;
ALTER TABLE catalog_items ADD COLUMN seeders      INTEGER;
ALTER TABLE catalog_items ADD COLUMN language     TEXT;
ALTER TABLE catalog_items ADD COLUMN released_at  TIMESTAMP;

-- Freshness ordering + window GC both scan by tracker upload time.
CREATE INDEX catalog_items_released_idx ON catalog_items(released_at);

-- One-shot reset. The population model changed (TMDB-discover seeding → tracker
-- RSS): the old metadata-only rows have availability='unknown' and no release
-- facts. catalog_items is a regenerable cache rebuilt by the freshness
-- scheduler within minutes of boot, so clear it and let the new pass repopulate
-- with availability='available' + release facts. (CASCADEs reco_feedback —
-- dismissals regenerate; harmless for a cache.)
DELETE FROM catalog_items;
