-- A title's own audio / subtitle choice (0042) now inherits the account-wide
-- one field by field when NULL. Clients used to copy the account values into
-- the title's row on every pick (a subtitles-off on a film also wrote the
-- account's audio language), so a field equal to the account value is such a
-- copy: cleared, it inherits the very same value. Rows left with no choice go.
UPDATE collection_playback_preferences
   SET audio_language = NULL
 WHERE audio_language = (SELECT p.audio_language FROM playback_preferences p
                          WHERE p.user_id = collection_playback_preferences.user_id);

UPDATE collection_playback_preferences
   SET subtitle_language = NULL
 WHERE subtitle_language = (SELECT p.subtitle_language FROM playback_preferences p
                             WHERE p.user_id = collection_playback_preferences.user_id);

DELETE FROM collection_playback_preferences
 WHERE audio_language IS NULL AND subtitle_language IS NULL;
