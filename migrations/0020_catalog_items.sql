-- Shared recommendation catalogue (Slice 2 of "For You").
--
-- TMDB (and later AniList) define the candidate universe; trackers are
-- the availability layer on top (Slice 3 flips `availability` to
-- 'available' once a provider can actually serve a candidate). The table
-- is household-shared — per-user filtering (language / genre / anime)
-- happens at request time in `reco.rs`, never at build time, so one
-- catalogue serves a multilingual household.
--
-- `genres` is a JSON array of TMDB genre ids (matches the user's
-- `genres` preference vocabulary). `original_language` is the TMDB
-- ISO 639-1 code ("fr"/"en"/…). `is_anime` marks the distinct Anime
-- category (NOT TMDB's "Animation" genre) — populated by the AniList
-- pass in Slice 4; TMDB-sourced rows stay 0. `availability` starts
-- 'unknown' and is confirmed by the tracker pass.

CREATE TABLE catalog_items (
    id                   BLOB PRIMARY KEY,
    tmdb_id              INTEGER,
    anilist_id           INTEGER,
    kind                 TEXT NOT NULL CHECK (kind IN ('movie', 'tv')),
    title                TEXT NOT NULL,
    original_language    TEXT,
    genres               TEXT NOT NULL DEFAULT '[]',
    is_anime             BOOLEAN NOT NULL DEFAULT 0,
    poster_path          TEXT,
    backdrop_path        TEXT,
    overview             TEXT,
    popularity           REAL,
    vote_average         REAL,
    release_date         TEXT,
    availability         TEXT NOT NULL DEFAULT 'unknown'
                             CHECK (availability IN ('unknown', 'available', 'imminent', 'unavailable')),
    available_provider   TEXT,
    available_checked_at TIMESTAMP,
    source               TEXT,
    first_seen_at        TIMESTAMP NOT NULL,
    last_refreshed_at    TIMESTAMP NOT NULL
);

-- Dedup key for TMDB-sourced (non-anime) rows. Scoped to is_anime = 0 so
-- an anime row keyed on anilist_id may carry the same tmdb_id (for
-- library/seen exclusion) without colliding with the TMDB row of the same
-- title. The matching upsert targets this exact predicate:
-- `ON CONFLICT(tmdb_id, kind) WHERE tmdb_id IS NOT NULL AND is_anime = 0`.
CREATE UNIQUE INDEX catalog_items_tmdb_kind_idx
    ON catalog_items(tmdb_id, kind) WHERE tmdb_id IS NOT NULL AND is_anime = 0;

-- Dedup key for AniList-sourced rows: anime identity is the anilist_id
-- (whether or not it reconciled to a tmdb_id).
CREATE UNIQUE INDEX catalog_items_anilist_idx
    ON catalog_items(anilist_id) WHERE anilist_id IS NOT NULL;

-- Browse path: "available <kind> (anime?) by popularity".
CREATE INDEX catalog_items_browse_idx
    ON catalog_items(kind, is_anime, availability, popularity);

-- Per-user language filtering at request time.
CREATE INDEX catalog_items_language_idx ON catalog_items(original_language);
