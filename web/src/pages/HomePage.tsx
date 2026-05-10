import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useNavigate } from "react-router";
import { Bookmark, Plus } from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import { Shelf } from "@/components/Shelf";
import { Badge } from "@/components/ui/badge";
import {
  discover,
  follows,
  me as meApi,
  tmdbImage,
  torrents,
  type ContinueWatchingItem,
  type FollowSummary,
  type SearchResult,
  type TorrentView,
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
    queryKey: ["follows"],
    queryFn: follows.list,
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
  const recentLibrary = useMemo(
    () => (libraryQ.data ?? []).slice(0, 12),
    [libraryQ.data],
  );

  return (
    <div className="grid gap-10">
      <Shelf
        title="Continue Watching"
        isEmpty={!continueQ.data || continueQ.data.length === 0}
        emptyState={
          <span>
            Once you start watching something, you'll find it here ready to
            resume.
          </span>
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
              Find a series in <Link to="/search" className="underline">search</Link> and click "Follow" to add it here.
            </span>
          </div>
        }
      >
        {watchlistQ.data?.map((f) => <WatchlistCard key={f.id} follow={f} />)}
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
            Nothing in the library yet. Start a <Link to="/search" className="underline">search</Link> to add your first title.
          </span>
        }
      >
        {recentLibrary.map((t) => <LibraryCard key={t.infohash} torrent={t} />)}
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
      tmdbId={item.tmdb_verified ? item.tmdb_id : null}
      kind={null}
      progress={progress}
      progressColor="bg-primary"
    />
  );
}

function WatchlistCard({ follow }: { follow: FollowSummary }) {
  // SCENE-mode: route by follow id, not tmdb_id. Poster only when
  // the server-side gate confirms a tmdb_verified collection.
  const href = `/series/${follow.id}`;
  return (
    <MediaCard
      href={href}
      title={follow.name}
      posterUrl={tmdbImage(follow.poster_path, "w342")}
      kind="tv"
      badge={
        follow.new_count > 0 ? (
          <Badge className="bg-primary text-primary-foreground shadow-md">
            {follow.new_count} new
          </Badge>
        ) : (
          <Bookmark className="size-3.5 text-white/80 drop-shadow" />
        )
      }
    />
  );
}

function FeaturedCard({ result }: { result: SearchResult }) {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const watchlistQ = useQuery({
    queryKey: ["follows"],
    queryFn: follows.list,
    staleTime: 60_000,
  });
  // Find an existing follow whose normalized name matches this
  // result's title — if so, the card jumps straight into it on
  // click.
  const existing = useMemo(() => {
    const norm = normalizeForMatch(result.title);
    return watchlistQ.data?.find((f) => f.normalized_name === norm);
  }, [watchlistQ.data, result.title]);

  const followMutation = useMutation({
    mutationFn: () =>
      follows.add(result.title, result.tmdb_id ?? null),
    onSuccess: (created) => {
      void qc.invalidateQueries({ queryKey: ["follows"] });
      if (result.kind === "tv") {
        navigate(`/series/${created.id}`);
      }
    },
  });

  const subtitle = [
    result.year,
    result.seeders != null ? `${result.seeders} seeders` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  // For TV: route to /series if already followed; otherwise the
  // primary CTA is a Follow button (which adds + navigates).
  // For movies: prefilled search (no follow concept for movies).
  const movieHref = `/search?q=${encodeURIComponent(result.title)}`;

  if (result.kind === "tv") {
    return (
      <div className="relative">
        <MediaCard
          href={existing ? `/series/${existing.id}` : undefined}
          onClick={
            existing
              ? undefined
              : () => {
                  if (!followMutation.isPending) followMutation.mutate();
                }
          }
          title={result.title}
          subtitle={subtitle || undefined}
          kind="tv"
          badge={
            existing ? (
              <Bookmark className="size-3.5 text-white/80 drop-shadow" />
            ) : (
              <Badge
                variant="secondary"
                className="bg-primary/80 text-[10px] uppercase text-primary-foreground shadow-md"
              >
                <Plus className="mr-0.5 size-3" /> Follow
              </Badge>
            )
          }
        />
      </div>
    );
  }
  return (
    <MediaCard
      href={movieHref}
      title={result.title}
      subtitle={subtitle || undefined}
      kind="movie"
      badge={
        result.freeleech ? (
          <Badge
            variant="secondary"
            className="bg-emerald-500/20 text-[10px] uppercase text-emerald-300 shadow-md"
          >
            FL
          </Badge>
        ) : undefined
      }
    />
  );
}

/// SCENE normalisation kept in sync with iris-media's normalize_title.
/// Used for client-side "do I already follow this?" lookups so we
/// don't double-follow the same series with different surface
/// titles. Match order of operations: lowercase → keep alnum →
/// collapse runs of non-alnum into single spaces → trim.
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
  return out.trim();
}


function LibraryCard({ torrent }: { torrent: TorrentView }) {
  const videos = torrent.files.filter((f) => VIDEO_RE.test(f.path));
  const href =
    videos.length === 1
      ? `/watch/${torrent.infohash}/${videos[0].index}`
      : "/library";
  const subtitle = formatSize(torrent.total_size_bytes);
  const downloading = torrent.progress_pct < 99.9;
  const progress = downloading ? torrent.progress_pct / 100 : undefined;
  return (
    <MediaCard
      href={href}
      title={torrent.name ?? torrent.infohash.slice(0, 12)}
      subtitle={subtitle}
      tmdbId={torrent.tmdb_verified ? torrent.tmdb_id : null}
      kind={null}
      progress={progress}
      progressColor="bg-sky-500"
    />
  );
}
