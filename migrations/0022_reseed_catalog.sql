-- One-shot catalogue reset.
--
-- The discovery seeding changed: movies are now gated to titles with a
-- past digital/physical release (no more theatrical-only entries from the
-- old trending / now_playing seeding). `catalog_items` is a regenerable
-- cache rebuilt by the reco scheduler within ~15 s of boot, so the
-- cleanest way to drop the stale theatrical rows is to clear it and let
-- the corrected pass repopulate. (CASCADEs `reco_feedback` — dismissals
-- are regenerated too; harmless for a cache.)

DELETE FROM catalog_items;
