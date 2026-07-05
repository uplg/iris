-- Per-user "removed from Continue Watching" for a whole series (collection).
-- A resume/next-up tile for a TV collection can't be dismissed by deleting a
-- single progress row: an earlier completed episode just becomes the new
-- frontier and the shelf regenerates the next-up tile. So a dismissal is
-- recorded at the COLLECTION level with a timestamp; the Continue Watching
-- queries hide a collection whose dismissal is at-or-after its latest
-- playback. Watching a NEWER episode (last_watched_at > dismissed_at) makes
-- the dismissal stale, so the series returns — matching the Netflix
-- "remove from continue watching" behaviour (hidden until you engage again).
--
-- Movies / standalone torrents (no collection) are not dismissed here — their
-- "remove" simply deletes the single progress row.
CREATE TABLE cw_dismissed (
    user_id       BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    collection_id BLOB      NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    dismissed_at  TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, collection_id)
);
