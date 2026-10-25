-- Films an admin shares with SéléoTV, a separate app on the same host that
-- mounts `downloads/` and `share/` of the data volume read-only. Iris writes
-- `share/seleo.json` from these rows, and the disk GC leaves a torrent alone
-- while one of its shares runs (`until` in the future). `path` is relative to
-- the download dir; the metadata is a snapshot taken at share time, so the
-- manifest is rebuilt without TMDB. `poster` is a file name under
-- `share/posters/`.
CREATE TABLE seleo_shares (
    infohash        TEXT      NOT NULL REFERENCES torrents(infohash) ON DELETE CASCADE,
    file_idx        INTEGER   NOT NULL,
    shared_by       BLOB      REFERENCES users(id) ON DELETE SET NULL,
    shared_at       TIMESTAMP NOT NULL,
    until           TIMESTAMP NOT NULL,
    path            TEXT      NOT NULL,
    size_bytes      INTEGER   NOT NULL,
    tmdb_id         INTEGER,
    title           TEXT      NOT NULL,
    year            INTEGER,
    overview        TEXT,
    runtime_minutes INTEGER,
    poster          TEXT,
    PRIMARY KEY (infohash, file_idx)
);

CREATE INDEX seleo_shares_until_idx ON seleo_shares(until);
