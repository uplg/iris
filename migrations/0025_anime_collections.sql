-- Anime-aware collection identity + absolute episode numbering.
--
-- One reported bug, two root causes. A user grabbed
-- `One Piece S01E1156 … -Tsundere-Raws (CR).mkv` and the Library page
-- amalgamated THREE distinct "one piece" entities — the fleuve fansub
-- anime (absolute episode crammed into a fake S01), the Netflix/CR
-- season re-cut, and the live-action 2023 series — because collection
-- identity was title-only. The scheduler's broad "One Piece" search
-- then dumped ~22 anime "seasons" + the live-action into one card,
-- overflowing the season-tab strip.
--
-- Fix 1 — identity: `is_anime` lets the assign path write an `anime:`
-- prefix into `parsed_title_normalized`, so the anime and the
-- live-action (same title) land in DISTINCT collections. The prefix
-- lives inside the existing key string, so the
-- `(parsed_title_normalized, kind)` unique index is untouched.
-- `anilist_id` is filled asynchronously by the AniList/TMDB confirm
-- step (enrichment only; never flips identity after creation).
--
-- Fix 2 — numbering: `absolute_episode` records the canonical absolute
-- number for genuinely-fleuve releases so the client can render one
-- flat ordered list ("Episode 1156") instead of a fake-season tab.
-- Whether a collection displays flat vs seasonal is DERIVED at read
-- time from the episode set — there is no `numbering` column.
--
-- All additive: existing rows keep working (NULL / 0 defaults), and the
-- shipped APKs (ignoreUnknownKeys / serde defaults) keep parsing.

ALTER TABLE collections        ADD COLUMN is_anime   BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE collections        ADD COLUMN anilist_id INTEGER;

ALTER TABLE episode_files      ADD COLUMN absolute_episode INTEGER;
ALTER TABLE available_episodes ADD COLUMN absolute_episode INTEGER;

-- Read pattern for the flat anime list: "every absolute episode for
-- this normalized anime series".
CREATE INDEX IF NOT EXISTS available_episodes_absolute_idx
    ON available_episodes(normalized_name, absolute_episode)
    WHERE absolute_episode IS NOT NULL;
