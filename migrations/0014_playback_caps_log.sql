-- Telemetry for client capability declarations sent via `Iris-Caps`.
-- See `docs/SOTA_ARCHITECTURE.md` §2.2.
--
-- Best-effort logging from the iris-api middleware on every
-- /api/torrents/.../{manifest.json,stream,seek,playback-error} request.
-- We keep raw JSON (rather than denormalising) so the schema can evolve
-- without further migrations; queries on the JSON columns are run ad-hoc.
CREATE TABLE playback_caps_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ts          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    infohash    TEXT,
    file_idx    INTEGER,
    route       TEXT,        -- e.g. "manifest.json", "stream", "playback-error"
    caps_json   TEXT,        -- serialised ClientCapabilities
    user_agent  TEXT,
    request_id  TEXT
);

CREATE INDEX idx_playback_caps_log_ts ON playback_caps_log(ts);
CREATE INDEX idx_playback_caps_log_infohash ON playback_caps_log(infohash);
