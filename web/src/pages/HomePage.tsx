import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Link } from "react-router";
import { Bookmark } from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import { PreviewDialog } from "@/components/PreviewDialog";
import { Shelf } from "@/components/Shelf";
import { Badge } from "@/components/ui/badge";
import {
  discover,
  me as meApi,
  tmdbImage,
  torrents,
  type ContinueWatchingItem,
  type SearchResult,
  type TorrentView,
  type WatchlistItem,
} from "@/lib/api";
import { formatSize } from "@/lib/format";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/**
 * Discovery-first home. Vertical stack of horizontal shelves — Continue
 * Watching, Watchlist, Featured movies/series, Library. The actual
 * search interface moved to its own /search route (kept simple here to
 * make the home about *picking something to watch*, not querying the
 * indexer).
 */
export function HomePage() {
  const continueQ = useQuery({
    queryKey: ["continue-watching"],
    queryFn: meApi.continueWatching,
    staleTime: 30_000,
  });
  const watchlistQ = useQuery({
    queryKey: ["watchlist"],
    queryFn: meApi.watchlist,
    staleTime: 60_000,
  });
  const featuredQ = useQuery({
    queryKey: ["discover-featured"],
    queryFn: discover.featured,
    staleTime: 5 * 60_000,
  });
  const libraryQ = useQuery({
    queryKey: ["torrents"],
    queryFn: torrents.list,
    refetchInterval: 5_000,
  });

  // Library shelf only shows the recent N — full grid lives at /library.
  const recentLibrary = useMemo(() => (libraryQ.data ?? []).slice(0, 12), [libraryQ.data]);

  return (
    <div className="grid gap-10">
      <Shelf
        title="Continue Watching"
        isEmpty={!continueQ.data || continueQ.data.length === 0}
        emptyState={
          <span>Once you start watching something, you'll find it here ready to resume.</span>
        }
      >
        {continueQ.data?.map((item) => (
          <ContinueCard key={`${item.infohash}:${item.file_idx}`} item={item} />
        ))}
      </Shelf>

      <Shelf
        title="My Watchlist"
        isEmpty={!watchlistQ.data || watchlistQ.data.length === 0}
        emptyState={
          <div className="grid gap-2">
            <span>No series followed yet.</span>
            <span className="text-xs">
              Find a series in{" "}
              <Link to="/search" className="underline">
                search
              </Link>{" "}
              and click "Follow" to add it here.
            </span>
          </div>
        }
      >
        {watchlistQ.data?.map((w) => (
          <WatchlistCard key={w.id} item={w} />
        ))}
      </Shelf>

      <Shelf
        title="New Movies"
        isEmpty={!featuredQ.data || featuredQ.data.movies.length === 0}
        emptyState={<span>No movie releases found yet.</span>}
      >
        {featuredQ.data?.movies.map((r) => (
          <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
        ))}
      </Shelf>

      <Shelf
        title="New Series"
        isEmpty={!featuredQ.data || featuredQ.data.series.length === 0}
        emptyState={<span>No series releases found yet.</span>}
      >
        {featuredQ.data?.series.map((r) => (
          <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
        ))}
      </Shelf>

      <Shelf
        title="My Library"
        href="/library"
        isEmpty={recentLibrary.length === 0}
        emptyState={
          <span>
            Nothing in the library yet. Start a{" "}
            <Link to="/search" className="underline">
              search
            </Link>{" "}
            to add your first title.
          </span>
        }
      >
        {recentLibrary.map((t) => (
          <LibraryCard key={t.infohash} torrent={t} />
        ))}
      </Shelf>
    </div>
  );
}

function ContinueCard({ item }: { item: ContinueWatchingItem }) {
  const fileName = item.file_path?.split("/").pop();
  const primary = fileName ?? item.torrent_name;
  const subtitle = fileName && fileName !== item.torrent_name ? item.torrent_name : undefined;
  const progress =
    item.duration_seconds && item.duration_seconds > 0
      ? Math.min(1, item.position_seconds / item.duration_seconds)
      : 0;

  return (
    <MediaCard
      href={`/watch/${item.infohash}/${item.file_idx}`}
      title={primary}
      subtitle={subtitle}
      tmdbId={item.tmdb_id}
      kind={item.kind}
      progress={progress}
      progressColor="bg-primary"
    />
  );
}

function WatchlistCard({ item }: { item: WatchlistItem }) {
  // Watchlist items carry the collection id directly — route to the
  // unified Series view. Per-user state (new_count, last_visited_at)
  // is computed server-side off the calling user's series_follows row.
  return (
    <MediaCard
      href={`/collection/${item.id}`}
      title={item.name}
      posterUrl={tmdbImage(item.poster_path, "w342")}
      kind="tv"
      badge={
        item.new_count > 0 ? (
          <Badge className="bg-primary text-primary-foreground shadow-md">
            {item.new_count} new
          </Badge>
        ) : (
          <Bookmark className="size-3.5 text-white/80 drop-shadow" />
        )
      }
    />
  );
}

function FeaturedCard({ result }: { result: SearchResult }) {
  // Detect whether the user already follows this series — only TV
  // results matter for the Watchlist bookmark badge. We DON'T
  // auto-follow on click anymore (was a usability foot-gun); the
  // explicit Follow action lives inside PreviewDialog.
  const watchlistQ = useQuery({
    queryKey: ["watchlist"],
    queryFn: meApi.watchlist,
    staleTime: 60_000,
  });
  const existing = useMemo(() => {
    if (result.kind !== "tv") return undefined;
    const norm = normalizeForMatch(result.title);
    return watchlistQ.data?.find((f) => f.normalized_name === norm);
  }, [watchlistQ.data, result.title, result.kind]);

  const [previewOpen, setPreviewOpen] = useState(false);

  const subtitle = [result.year, result.seeders != null ? `${result.seeders} seeders` : null]
    .filter(Boolean)
    .join(" · ");

  // If the household already has this series in their Watchlist
  // (= a collection with on-disk episodes), the card body links
  // straight to the unified collection view (skips the dialog
  // round-trip). Otherwise the click opens PreviewDialog where the
  // Play / Download CTA lives.
  const cardOnClick = existing ? undefined : () => setPreviewOpen(true);
  const cardHref = existing ? `/collection/${existing.id}` : undefined;

  return (
    <>
      <MediaCard
        href={cardHref}
        onClick={cardOnClick}
        title={result.title}
        subtitle={subtitle || undefined}
        posterUrl={result.poster_url}
        kind={result.kind}
        badge={
          existing ? (
            <Bookmark className="size-3.5 text-white/80 drop-shadow" />
          ) : result.freeleech ? (
            <Badge
              variant="secondary"
              className="bg-emerald-500/20 text-[10px] uppercase text-emerald-300 shadow-md"
            >
              FL
            </Badge>
          ) : undefined
        }
      />
      <PreviewDialog
        open={previewOpen}
        onOpenChange={setPreviewOpen}
        providerId={result.provider_id}
        externalId={result.external_id}
        initialTitle={result.title}
        tmdbId={result.tmdb_id}
      />
    </>
  );
}

/// SCENE normalisation kept in sync with iris-media's
/// `normalize_title` + the TV-side trailing-year strip from
/// `Parsed::collection_key(true)`. Used for client-side "do I
/// already follow this?" lookups so we don't double-follow the
/// same series with different surface titles, and so a card
/// titled "Lucky Luke" matches a follow whose underlying SCENE
/// torrents normalise to "lucky luke 1991".
function normalizeForMatch(s: string): string {
  let out = "";
  let lastSpace = true;
  for (const c of s) {
    if (/[a-z0-9]/i.test(c)) {
      out += c.toLowerCase();
      lastSpace = false;
    } else if (!lastSpace) {
      out += " ";
      lastSpace = true;
    }
  }
  return stripTrailingYear(out.trim());
}

function stripTrailingYear(s: string): string {
  const m = /^(.*) (\d{4})$/.exec(s);
  if (!m) return s;
  const y = parseInt(m[2], 10);
  if (y < 1900 || y > 2099) return s;
  return m[1];
}

function LibraryCard({ torrent }: { torrent: TorrentView }) {
  const videos = torrent.files.filter((f) => VIDEO_RE.test(f.path));
  const href = videos.length === 1 ? `/watch/${torrent.infohash}/${videos[0].index}` : "/library";
  const subtitle = formatSize(torrent.total_size_bytes);
  const downloading = torrent.progress_pct < 99.9;
  const progress = downloading ? torrent.progress_pct / 100 : undefined;
  return (
    <MediaCard
      href={href}
      title={torrent.name ?? torrent.infohash.slice(0, 12)}
      subtitle={subtitle}
      tmdbId={torrent.tmdb_id}
      kind={torrent.kind}
      progress={progress}
      progressColor="bg-sky-500"
    />
  );
}
