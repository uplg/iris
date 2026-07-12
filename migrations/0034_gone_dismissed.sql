-- Per-user "hide this Gone entry" — the ghost/gone surfaces are derived
-- from the caller's own watch history and the collection's reclaimed
-- torrents, so users need a way to make an entry disappear WITHOUT
-- erasing history (playback_progress is never touched; the History page
-- keeps every row by design).
--
-- Same timestamped-staleness model as `cw_dismissed` (0032): a dismissal
-- only hides an entry while it is at-or-after the entry's latest
-- activity. Newer activity makes it stale and the entry returns:
--
--   * a release re-downloaded then reclaimed AGAIN gets a fresh
--     `deleted_at` > `dismissed_at` → its gone rows reappear;
--   * watching something new in a ghost collection bumps the user's
--     `last_watched_at` past `dismissed_at` → the ghost card returns
--     to their Library.

-- One reclaimed release hidden from the caller's gone surfaces on the
-- collection page (both the per-episode gone rows and the raw release
-- row). Keyed by infohash (torrents.infohash is UNIQUE; rows are only
-- ever soft-deleted, so the FK stays valid).
CREATE TABLE gone_release_dismissed (
    user_id      BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    infohash     TEXT      NOT NULL REFERENCES torrents(infohash) ON DELETE CASCADE,
    dismissed_at TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, infohash)
);

-- A whole ghost collection hidden from the caller's Library grid.
-- Does NOT affect the collection page itself (still navigable from
-- History) nor anyone else's Library — ghosts are per-user already.
CREATE TABLE ghost_dismissed (
    user_id       BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    collection_id BLOB      NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    dismissed_at  TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, collection_id)
);
