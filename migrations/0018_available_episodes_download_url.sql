-- Persisted download URL on cached indexer offers.
--
-- Background: Torznab providers (c411, generic) ship pre-signed
-- `.torrent` URLs in the `<link>` of each RSS item. The provider
-- stashes them in an in-memory `link_cache` at search time; at grab
-- time `resolve()` reads back from that cache.
--
-- Problem: the cache is process-local. Every server restart wipes
-- it. c411 paginates its results so a release first cached weeks
-- ago is no longer in the top page when the scheduler re-runs — the
-- URL is gone forever from the cache's point of view. `resolve()`
-- then 500s with "no cached download URL for X".
--
-- Fix: persist the URL on the `available_episodes` row alongside
-- everything else the scheduler captured. The grab path checks
-- this column first and only falls back to `provider.resolve()`
-- when null (legacy rows, or providers that don't expose a URL —
-- torr9's JSON API is the main one).

ALTER TABLE available_episodes ADD COLUMN download_url TEXT;
