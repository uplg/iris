-- Discovery + series-tracking foundations.
--
-- Four new tables and one new column on `torrents`:
--
--   * `series_follows`     — per-user "I'm watching this show" entries.
--                            Drives the Watchlist shelf and tells the
--                            notify scheduler which TMDB ids to poll.
--   * `episode_files`      — library-wide mapping
--                            `(tmdb_id, season, episode) → (infohash, file_idx)`.
--                            Populated at ingest time (TMDB match + SCENE
--                            parse of filenames) AND by the on-demand grab
--                            endpoint. NOT per-user — the file lives on
--                            disk once and is visible to every authorised
--                            user. "Did I watch this" stays in
--                            `playback_progress`.
--   * `available_episodes` — global cache of indexer hits for episodes
--                            that aren't in the library yet. Filled by
--                            the 4-hourly notify scheduler so a user
--                            click on "Préparer E06" doesn't have to
--                            wait on a fresh indexer query.
--   * `collections`        — logical grouping of one or more torrents
--                            into a single library entity (typically a
--                            TV show, sometimes a movie + its extras).
--                            Two paths to identity: `tmdb_id` when we
--                            have a verified TMDB match, otherwise a
--                            normalised `parsed_title` extracted from
--                            SCENE filenames.
--   * `torrents.collection_id` — back-reference. NULL until the
--                            collection assignment job runs (Phase 4.5).

-- ---------------------------------------------------------------------------
-- series_follows: user → TMDB show
-- ---------------------------------------------------------------------------

CREATE TABLE series_follows (
    id                  BLOB    PRIMARY KEY NOT NULL,
    user_id             BLOB    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tmdb_id             INTEGER NOT NULL,
    -- Snapshot of the show name at follow time. TMDB names occasionally
    -- get retitled — keeping a snapshot avoids confusing UI "wait, why
    -- did my Squid Game follow turn into 오징어 게임".
    name                TEXT    NOT NULL,
    total_seasons       INTEGER,
    -- Last time the notify scheduler hit TMDB + indexer for this follow.
    -- Used to skip recent ones on each tick (cheap pacing).
    last_checked_at     TIMESTAMP,
    -- Last time the user opened the series detail page. Powers the
    -- "X nouveaux" badge on Watchlist cards: count of episodes whose
    -- found_at > last_visited_at.
    last_visited_at     TIMESTAMP,
    created_at          TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX series_follows_user_tmdb_idx
    ON series_follows(user_id, tmdb_id);

CREATE INDEX series_follows_scheduler_idx
    ON series_follows(last_checked_at);

-- ---------------------------------------------------------------------------
-- episode_files: library-wide (tmdb_id, S, E) → (infohash, file_idx)
-- ---------------------------------------------------------------------------

CREATE TABLE episode_files (
    id              BLOB    PRIMARY KEY NOT NULL,
    tmdb_id         INTEGER NOT NULL,
    season          INTEGER NOT NULL,
    episode         INTEGER NOT NULL,
    infohash        TEXT    NOT NULL,
    file_idx        INTEGER NOT NULL,
    -- How we figured out the (S, E) for this file:
    --   'tmdb_match'  — TMDB episode lookup by air date / runtime
    --   'scene_parse' — regex on the filename (Show.Name.S01E02.…)
    --   'manual'      — user assigned via the admin UI (future)
    derived_from    TEXT    NOT NULL CHECK (derived_from IN ('tmdb_match', 'scene_parse', 'manual')),
    created_at      TIMESTAMP NOT NULL,
    -- Same physical file shouldn't be claimed twice.
    UNIQUE (infohash, file_idx)
);

-- Read pattern: "give me everything we have for Squid Game S2".
CREATE INDEX episode_files_lookup_idx
    ON episode_files(tmdb_id, season, episode);

-- ---------------------------------------------------------------------------
-- available_episodes: indexer pre-cache for "Préparer E06" clicks
-- ---------------------------------------------------------------------------

CREATE TABLE available_episodes (
    id                      BLOB    PRIMARY KEY NOT NULL,
    tmdb_id                 INTEGER NOT NULL,
    season                  INTEGER NOT NULL,
    episode                 INTEGER NOT NULL,
    indexer_provider        TEXT    NOT NULL,
    indexer_torrent_id      TEXT    NOT NULL,
    -- Cached at find time so the on-demand grab doesn't have to re-query
    -- the indexer details endpoint. Magnets are stable.
    magnet                  TEXT    NOT NULL,
    -- Quality hints for the picker: "1080p", "WEB-DL", seeders count.
    -- All optional — different providers expose different shapes.
    quality                 TEXT,
    seeders                 INTEGER,
    size_bytes              INTEGER,
    found_at                TIMESTAMP NOT NULL
);

-- One row per (episode × provider × specific torrent). Multiple torrents
-- per episode is fine (different qualities, re-uploads) — the picker
-- ranks them at grab time.
CREATE UNIQUE INDEX available_episodes_dedup_idx
    ON available_episodes(tmdb_id, season, episode, indexer_provider, indexer_torrent_id);

-- Read pattern: "what's grabbable for Squid Game S2".
CREATE INDEX available_episodes_lookup_idx
    ON available_episodes(tmdb_id, season, episode);

-- ---------------------------------------------------------------------------
-- collections: logical grouping spanning N torrents
-- ---------------------------------------------------------------------------

CREATE TABLE collections (
    id                      BLOB    PRIMARY KEY NOT NULL,
    -- Preferred identity. Verified TMDB id (we already gate `tmdb_id` on
    -- runtime match at ingest, so this is trustworthy when present).
    tmdb_id                 INTEGER,
    -- Fallback identity for SCENE-named multi-torrent series with no TMDB
    -- hit. Stored normalised (lowercase, no punctuation, single spaces)
    -- so the UNIQUE index actually catches grouping intents.
    parsed_title_normalized TEXT,
    -- Display version of the title (kept for UI). Either the TMDB name
    -- or the SCENE-extracted "Show Name" with original casing.
    display_title           TEXT    NOT NULL,
    kind                    TEXT    NOT NULL CHECK (kind IN ('tv', 'movie')),
    created_at              TIMESTAMP NOT NULL
);

-- One collection per TMDB entity (when we have a tmdb match).
-- SQLite treats NULLs as distinct so partial-on-NOT-NULL is the right
-- shape — collections without tmdb_id can coexist freely.
CREATE UNIQUE INDEX collections_tmdb_idx
    ON collections(tmdb_id) WHERE tmdb_id IS NOT NULL;

-- One collection per (normalised-title, kind) for the no-TMDB path.
CREATE UNIQUE INDEX collections_parsed_title_idx
    ON collections(parsed_title_normalized, kind)
    WHERE parsed_title_normalized IS NOT NULL;

-- ---------------------------------------------------------------------------
-- torrents.collection_id: back-reference
-- ---------------------------------------------------------------------------

-- Filled by the assignment job (Phase 4.5) at ingest time and via a
-- one-shot retroactive batch on the existing library. ON DELETE SET NULL
-- so removing a collection re-orphans its torrents instead of cascading
-- the deletion (collections are cheap, torrents aren't).
ALTER TABLE torrents ADD COLUMN collection_id BLOB
    REFERENCES collections(id) ON DELETE SET NULL;

CREATE INDEX torrents_collection_idx
    ON torrents(collection_id) WHERE collection_id IS NOT NULL;
