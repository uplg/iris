-- TMDB trust gate. A collection's `tmdb_id` (poster, synopsis, titles on
-- every surface) is written only when a trusted signal backs it, and
-- `tmdb_trust` records which one:
--
--   'admin'          an admin set it (reserved: no route writes it yet)
--   'tracker_scene'  the tracker's own id and the strict SCENE match agree
--   'scene'          strict SCENE match only (exact normalised title, kind, year)
--   'tracker'        tracker id only, kind matches and year within ±1
--   NULL             legacy row, not evaluated yet: keeps the old behaviour
--                    until `tmdb-trust --apply` re-evaluates it
--
-- Additive: no existing row changes meaning until the apply tool runs.
ALTER TABLE collections ADD COLUMN tmdb_trust TEXT
    CHECK (tmdb_trust IN ('admin', 'tracker_scene', 'scene', 'tracker'));

-- `torrents.tmdb_id` is revived to hold the id the TRACKER shipped for the
-- release (trust signal T1). Its legacy values are of mixed provenance
-- (whatever the client sent at grab time, often our own fuzzy SCENE guess),
-- so they stay untouched but unmarked: only rows with
-- `tmdb_id_source = 'tracker'` count as tracker ids.
ALTER TABLE torrents ADD COLUMN tmdb_id_source TEXT
    CHECK (tmdb_id_source IN ('tracker'));
