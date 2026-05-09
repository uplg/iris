import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link } from "react-router";
import { Bookmark } from "lucide-react";

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
        title="Ma Watchlist"
        isEmpty={!watchlistQ.data || watchlistQ.data.length === 0}
        emptyState={
          <div className="grid gap-2">
            <span>Aucune série suivie pour l'instant.</span>
            <span className="text-xs">
              Trouve une série dans la <Link to="/search" className="underline">recherche</Link> et clique sur "Suivre" pour l'ajouter ici.
            </span>
          </div>
        }
      >
        {watchlistQ.data?.map((f) => <WatchlistCard key={f.tmdb_id} follow={f} />)}
      </Shelf>

      <Shelf
        title="Sorties Ciné"
        isEmpty={!featuredQ.data || featuredQ.data.movies.length === 0}
        emptyState={<span>Aucune sortie cinéma trouvée pour l'instant.</span>}
      >
        {featuredQ.data?.movies.map((r) => (
          <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
        ))}
      </Shelf>

      <Shelf
        title="Sorties Séries"
        isEmpty={!featuredQ.data || featuredQ.data.series.length === 0}
        emptyState={<span>Aucune sortie série trouvée pour l'instant.</span>}
      >
        {featuredQ.data?.series.map((r) => (
          <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
        ))}
      </Shelf>

      <Shelf
        title="Ma Bibliothèque"
        href="/library"
        isEmpty={recentLibrary.length === 0}
        emptyState={
          <span>
            Rien encore en bibliothèque. Lance une <Link to="/search" className="underline">recherche</Link> pour ajouter ton premier titre.
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
  const href = `/series/${follow.tmdb_id}`;
  return (
    <MediaCard
      href={href}
      title={follow.name}
      subtitle={
        follow.total_seasons
          ? `${follow.total_seasons} saison${follow.total_seasons > 1 ? "s" : ""}`
          : undefined
      }
      posterUrl={tmdbImage(follow.poster_path, "w342")}
      kind="tv"
      badge={
        follow.new_count > 0 ? (
          <Badge className="bg-primary text-primary-foreground shadow-md">
            {follow.new_count} nouveau{follow.new_count > 1 ? "x" : ""}
          </Badge>
        ) : (
          <Bookmark className="size-3.5 text-white/80 drop-shadow" />
        )
      }
    />
  );
}

function FeaturedCard({ result }: { result: SearchResult }) {
  // TV shows with a TMDB id can route straight to the series page —
  // there the user can Follow + browse episodes. Movies (or TV shows
  // with no tmdb_id) fall back to a prefilled search so the indexer
  // results show up immediately.
  const href =
    result.kind === "tv" && result.tmdb_id != null
      ? `/series/${result.tmdb_id}`
      : `/search?q=${encodeURIComponent(result.title)}`;
  const subtitle = [
    result.year,
    result.seeders != null ? `${result.seeders} seeders` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <MediaCard
      href={href}
      title={result.title}
      subtitle={subtitle || undefined}
      tmdbId={result.tmdb_id}
      kind={result.kind}
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
