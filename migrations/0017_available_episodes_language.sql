-- Language dimension on cached indexer offers.
--
-- The household isn't single-language anymore: 2 anglophone users
-- vs ~8 francophone ones on the same library. The previous design
-- — "best per (S, E)" — forced one language to win the dedup, so
-- the anglophone half saw nothing grabbable on shows the
-- francophone half watched first, even when Seedpool had an
-- English release sitting right there.
--
-- New shape: one row per (S, E, language). The scheduler stops
-- filtering and stores up to three rows per episode (FR / EN /
-- MULTI / UNKNOWN, see iris_media::filename::Language). The UI
-- renders a language badge on each row so the user picks
-- explicitly — no global content-language preference, no implicit
-- "you get whatever the household majority gets".
--
-- `language` stays nullable so legacy rows survive — they read as
-- Unknown which the matcher already treats as "English-compatible".

ALTER TABLE available_episodes ADD COLUMN language TEXT;

-- The unique constraint stays `(normalized_name, season, episode,
-- indexer_provider, indexer_torrent_id)` — language is derived from
-- the title so it doesn't need its own slot in the key. Distinct
-- torrents on the same (S, E, provider) are already permitted by
-- the existing index and that's where language variation enters.

-- Useful when filtering "what FR offers do we have for this show".
CREATE INDEX IF NOT EXISTS available_episodes_language_idx
    ON available_episodes(normalized_name, season, episode, language);
