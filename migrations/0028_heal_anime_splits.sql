-- Heal anime/non-anime classification splits.
--
-- A handful of titles ended up with BOTH an anime row (is_anime=1, anilist_id —
-- the canonical identity, AniList artwork) AND a plain row (is_anime=0) for the
-- SAME tmdb_id. Cause: an earlier ingest resolved releases by their *file* title
-- instead of the release / collection title, so one pass missed the (genre 16 +
-- Japanese) anime gate and wrote a non-anime row alongside the anime one. The
-- resolution now derives from collection.tmdb_id, so this is a one-shot cleanup
-- of stale artifacts.
--
-- Same (tmdb_id, kind) = same work, so collapse to the anime row (richer:
-- anilist_id + AniList poster/banner + the correct dedup space). The anime row
-- also carries its own release facts, so grabbability is preserved. The match is
-- scoped to the SAME kind so a genuine movie/tv id-collision (TMDB's movie and tv
-- id namespaces overlap — different works sharing a number) is never touched.
-- catalog_items is a regenerable cache (CASCADEs reco_feedback dismissals, which
-- regenerate), so the delete is safe. The source path (upsert_anime) now enforces
-- this invariant on every write, so splits don't recur.
DELETE FROM catalog_items
WHERE is_anime = 0
  AND tmdb_id IS NOT NULL
  AND EXISTS (
    SELECT 1 FROM catalog_items a
    WHERE a.is_anime = 1
      AND a.tmdb_id = catalog_items.tmdb_id
      AND a.kind = catalog_items.kind
  );
