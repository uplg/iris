-- Server-side cache for TMDB multi-search results, keyed by the
-- SCENE-cleaned title. Two roles:
--
--   1. Avoid hammering TMDB on every search-result render — the web /
--      TV `ResultCard` resolves the poster from the cleaned release
--      name, and a fresh `Silicon Valley` search would otherwise issue
--      one upstream call per page render per client.
--   2. Power the ingestion-time `tmdb_id` override. Indexer-supplied
--      `tmdb_id`s on torr9 results are unreliable (Silicon Valley
--      releases pointed at "The Burning Bed", etc.). At ingest we
--      re-resolve from the cleaned torrent name and prefer the result
--      we get here over whatever the indexer attached.
--
-- The primary key is `(cleaned_name, kind_hint)` so the same query can
-- carry separate cached entries for the movie vs TV interpretation.
-- `tmdb_id IS NULL` means "TMDB returned nothing" (negative cache so we
-- don't retry on every page render).
CREATE TABLE tmdb_resolve_cache (
    cleaned_name   TEXT NOT NULL,
    kind_hint      TEXT,                 -- "movie" | "tv" | NULL
    tmdb_id        INTEGER,
    title          TEXT,
    year           INTEGER,
    poster_path    TEXT,
    backdrop_path  TEXT,
    overview       TEXT,
    fetched_at     TIMESTAMP NOT NULL,
    PRIMARY KEY (cleaned_name, kind_hint)
);

CREATE INDEX tmdb_resolve_cache_fetched_idx ON tmdb_resolve_cache(fetched_at);
