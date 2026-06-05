-- Per-user playback preferences: preferred audio + subtitle LANGUAGE.
--
-- Distinct from `playback_progress` (which stores the exact audio/subtitle
-- track *index* chosen for one specific file): track indices don't carry to
-- the next episode (a different file lists tracks in a different order), so
-- "keep my French audio + English subs across episodes" needs a
-- language-keyed, per-user preference applied by matching, not by index.
--
-- Layering at playback time: the file's own saved track index wins (most
-- specific), else this language preference (matched against the file's
-- tracks), else the file's default. A language that isn't present in a given
-- file falls back gracefully (audio → default, subtitles → off).
--
-- A dedicated table + endpoint (not columns on `user_preferences`) keeps the
-- existing full-replace `/api/me/preferences` PUT — used by shipped 0.5.x
-- clients — from ever resetting these fields.
--
--   * `audio_language`    — ISO 639-1 / BCP-47 (e.g. "fr", "en"); NULL = no
--                           preference (use the file's default audio).
--   * `subtitle_language` — same, OR the sentinel 'off' meaning "no
--                           subtitles"; NULL = no preference (file default).
-- Volume is intentionally NOT stored here: it's device-specific (web persists
-- it locally; Android TV uses the system/hardware volume).

CREATE TABLE playback_preferences (
    user_id            BLOB      PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    audio_language     TEXT,
    subtitle_language  TEXT,
    updated_at         TIMESTAMP NOT NULL
);
