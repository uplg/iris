-- Per-user, per-file playback state. Lets users resume where they left off,
-- remembers their audio/subtitle picks, and powers the "Continue watching"
-- shelf on the home page.

CREATE TABLE playback_progress (
    user_id             BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    infohash            TEXT NOT NULL,
    file_idx            INTEGER NOT NULL,
    position_seconds    REAL NOT NULL DEFAULT 0,
    duration_seconds    REAL,
    audio_track_idx     INTEGER,
    subtitle_track_idx  INTEGER,
    completed           BOOLEAN NOT NULL DEFAULT FALSE,
    last_watched_at     TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, infohash, file_idx)
);

CREATE INDEX playback_progress_user_recent
    ON playback_progress(user_id, last_watched_at DESC)
    WHERE completed = FALSE;
