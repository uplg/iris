-- Iris torrent ledger. The librqbit session keeps its own runtime state via
-- the JSON persistence file; this table records ownership, provenance and
-- the bookkeeping the GC layer (M5) will need.

CREATE TABLE torrents (
    id                      BLOB    PRIMARY KEY NOT NULL,
    infohash                TEXT    NOT NULL UNIQUE,
    name                    TEXT    NOT NULL,
    total_size_bytes        INTEGER NOT NULL,
    source_provider         TEXT,
    source_external_id      TEXT,
    added_by                BLOB    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    added_at                TIMESTAMP NOT NULL,
    last_played_at          TIMESTAMP,
    last_seed_activity_at   TIMESTAMP,
    deleted_at              TIMESTAMP
);

CREATE INDEX torrents_active_idx ON torrents(deleted_at, last_played_at);
CREATE INDEX torrents_added_by_idx ON torrents(added_by);
