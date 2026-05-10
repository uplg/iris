-- Pivot Watchlist follows to a SCENE-keyed identity. The previous
-- schema keyed follows on `tmdb_id` and pulled the canonical episode
-- list from the TMDB API per request — both unreliable when the
-- indexer mis-tags a torrent OR when TMDB returns the wrong show
-- for an ambiguous lookup.
--
-- New identity: `normalized_name` (the SCENE-style normalised
-- title — lowercase, punctuation-stripped, single spaces). This
-- joins directly to `collections.parsed_title_normalized`, which
-- is itself derived from SCENE filename parsing — one source of
-- truth all the way through.
--
-- TMDB stays as decoration only: stored on the follow when known,
-- but only USED (poster lookup) once `tmdb_verified` flips on a
-- collection torrent.

-- ---------------------------------------------------------------------------
-- series_follows: re-key on normalized_name
-- ---------------------------------------------------------------------------

DROP TABLE series_follows;

CREATE TABLE series_follows (
    id                  BLOB    PRIMARY KEY NOT NULL,
    user_id             BLOB    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- SCENE-normalised name — the join key against
    -- `collections.parsed_title_normalized`. Computed server-side
    -- from `name`; never trust client-supplied values.
    normalized_name     TEXT    NOT NULL,
    -- Display name as the user knows it (typically the TMDB or
    -- indexer title from the card they clicked). Used for the
    -- indexer search query inside the notify scheduler.
    name                TEXT    NOT NULL,
    -- Decoration-only TMDB id. Surfaces a poster when the
    -- collection it joins to is `tmdb_verified`. Never required.
    tmdb_id             INTEGER,
    -- Last time the notify scheduler hit the indexer for this
    -- follow. Used to skip recent ones on each tick.
    last_checked_at     TIMESTAMP,
    -- Last time the user opened the series detail page. Powers
    -- the "X nouveaux" badge on Watchlist cards.
    last_visited_at     TIMESTAMP,
    created_at          TIMESTAMP NOT NULL
);

-- Identity: one follow per (user, normalized_name).
CREATE UNIQUE INDEX series_follows_user_name_idx
    ON series_follows(user_id, normalized_name);

CREATE INDEX series_follows_scheduler_idx
    ON series_follows(last_checked_at);

-- ---------------------------------------------------------------------------
-- available_episodes: re-key on normalized_name
-- ---------------------------------------------------------------------------

DROP TABLE available_episodes;

CREATE TABLE available_episodes (
    id                      BLOB    PRIMARY KEY NOT NULL,
    -- SCENE-normalised name of the series. Joins to
    -- `series_follows.normalized_name`. NOT a tmdb_id.
    normalized_name         TEXT    NOT NULL,
    season                  INTEGER NOT NULL,
    episode                 INTEGER NOT NULL,
    indexer_provider        TEXT    NOT NULL,
    indexer_torrent_id      TEXT    NOT NULL,
    -- Cached at find time so the on-demand grab doesn't have to
    -- re-query the indexer details endpoint. Magnets are stable.
    -- Empty string allowed — providers that hand back .torrent
    -- files (torr9) defer resolution to the grab path.
    magnet                  TEXT    NOT NULL,
    quality                 TEXT,
    seeders                 INTEGER,
    size_bytes              INTEGER,
    found_at                TIMESTAMP NOT NULL
);

-- One row per (episode × provider × specific torrent).
CREATE UNIQUE INDEX available_episodes_dedup_idx
    ON available_episodes(normalized_name, season, episode, indexer_provider, indexer_torrent_id);

CREATE INDEX available_episodes_lookup_idx
    ON available_episodes(normalized_name, season, episode);
