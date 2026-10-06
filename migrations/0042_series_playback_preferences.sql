-- Per-series audio + subtitle language: "kept for the whole series". Same
-- shape and meaning as `playback_preferences` (0024), keyed by collection;
-- a series without a row falls back to the account-wide preference.
CREATE TABLE collection_playback_preferences (
    user_id            BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    collection_id      BLOB      NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    audio_language     TEXT,
    subtitle_language  TEXT,
    updated_at         TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, collection_id)
);
