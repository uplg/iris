-- Discovery "pulse": what the outside world is watching right now (TMDB
-- trending / French digital releases / on the air / curated moods, SIMKL
-- trending), joined against our own trackers by the pulse scheduler.
--
-- `pulse_signals` holds the latest snapshot of each external list, replaced
-- wholesale per (list, kind) on every pulse cycle. `list` is `trending`,
-- `simkl`, `digital`, `on_air` or `mood:<id>`.
CREATE TABLE pulse_signals (
    list       TEXT    NOT NULL,
    kind       TEXT    NOT NULL CHECK (kind IN ('movie', 'tv')),
    tmdb_id    INTEGER NOT NULL,
    -- 0-based position in the source list (lower = hotter).
    rank       INTEGER NOT NULL,
    -- SIMKL only: viewers who watched it today.
    watched    INTEGER,
    -- Localized (fr-FR) title from the list, a second search key for
    -- francophone trackers.
    title      TEXT    NOT NULL,
    -- `YYYY-MM-DD` release / first-air date as the list reported it.
    release_date TEXT,
    fetched_at TEXT    NOT NULL,
    PRIMARY KEY (list, kind, tmdb_id)
);
CREATE INDEX pulse_signals_title_idx ON pulse_signals(tmdb_id, kind);

-- Last tracker join attempt per title, so a budgeted cycle doesn't re-search
-- a title it just looked for. A miss is never written to `catalog_items`.
CREATE TABLE pulse_checks (
    tmdb_id    INTEGER NOT NULL,
    kind       TEXT    NOT NULL CHECK (kind IN ('movie', 'tv')),
    checked_at TEXT    NOT NULL,
    found      INTEGER NOT NULL,
    PRIMARY KEY (tmdb_id, kind)
);

-- The embedding recommender is gone: drop its vectors and the lazy
-- (never tracker-confirmed) candidates it created. Every remaining row is a
-- release a tracker actually carries.
DROP INDEX IF EXISTS catalog_items_embedding_model_idx;
ALTER TABLE catalog_items DROP COLUMN content_embedding;
ALTER TABLE catalog_items DROP COLUMN embedding_model;
DELETE FROM catalog_items WHERE availability <> 'available';
