import { useQuery } from "@tanstack/react-query";
import { type ReactNode, useMemo, useState } from "react";
import { Link } from "react-router";
import { ArrowUpRight, Bookmark, Play, Sparkles } from "lucide-react";

import { Container } from "@/components/Container";
import { MediaCard } from "@/components/MediaCard";
import { PreviewDialog } from "@/components/PreviewDialog";
import { Shelf } from "@/components/Shelf";
import { Tag } from "@/components/Tag";
import { Button } from "@/components/ui/button";
import {
  discover,
  me as meApi,
  metadata,
  tmdbImage,
  torrents,
  type ContinueWatchingItem,
  type SearchResult,
  type TorrentView,
  type WatchlistItem,
} from "@/lib/api";
import { formatSize, prettySceneName } from "@/lib/format";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/**
 * Discovery-first home. A full-bleed hero (resume the top Continue-Watching
 * pick, or the freshest featured release) over a vertical stack of
 * horizontal shelves — Continue Watching, Watchlist, Featured movies/series,
 * Library. The search interface lives on its own /search route.
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

  const resumePick = continueQ.data?.[0];
  // Hero fallback: the freshest library title reads better than a raw
  // featured release (verified TMDB art/name, no overflow), so prefer it.
  // Featured stays as the last resort for a brand-new, empty library.
  const libraryPick = libraryQ.data?.[0];
  const featuredPick = featuredQ.data?.movies[0] ?? featuredQ.data?.series[0];

  return (
    <div>
      {resumePick ? (
        <ResumeHero item={resumePick} />
      ) : libraryPick ? (
        <LibraryHero torrent={libraryPick} />
      ) : featuredPick ? (
        <FeaturedHero result={featuredPick} />
      ) : null}

      <Container>
        <div className="lanes pt-2">
          <Shelf
            eyebrow="For you"
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
            eyebrow="Following"
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
            eyebrow="Fresh"
            title="New Movies"
            isEmpty={!featuredQ.data || featuredQ.data.movies.length === 0}
            emptyState={<span>No movie releases found yet.</span>}
          >
            {featuredQ.data?.movies.map((r) => (
              <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
            ))}
          </Shelf>

          <Shelf
            eyebrow="Fresh"
            title="New Series"
            isEmpty={!featuredQ.data || featuredQ.data.series.length === 0}
            emptyState={<span>No series releases found yet.</span>}
          >
            {featuredQ.data?.series.map((r) => (
              <FeaturedCard key={`${r.provider_id}:${r.external_id}`} result={r} />
            ))}
          </Shelf>

          <Shelf
            eyebrow="On disk"
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
      </Container>
    </div>
  );
}

// ── HERO ───────────────────────────────────────────────────────────────────

function HeroLayout({
  eyebrow,
  backdropUrl,
  title,
  titlePending,
  meta,
  overview,
  actions,
  footer,
}: {
  eyebrow: ReactNode;
  backdropUrl: string | null;
  title: string;
  /** While the TMDB title is still resolving we show a skeleton bar
   *  rather than the raw release name — a giant unbreakable SCENE token
   *  overflows the hero and looks broken for the half-second it's up. */
  titlePending?: boolean;
  meta: string[];
  overview?: string | null;
  actions: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <section className="relative isolate mb-8" style={{ minHeight: "min(64vh, 640px)" }}>
      <div className="absolute inset-0 -z-10 overflow-hidden">
        {backdropUrl ? (
          <img
            src={backdropUrl}
            alt=""
            className="h-full w-full object-cover opacity-50"
            style={{ filter: "saturate(1.05)" }}
          />
        ) : (
          <div className="poster-fallback h-full w-full" />
        )}
        <div
          className="absolute inset-0"
          style={{
            background:
              "linear-gradient(180deg, oklch(0 0 0 / 0) 0%, var(--background) 92%), linear-gradient(90deg, var(--background) 0%, oklch(0 0 0 / 0.4) 60%, oklch(0 0 0 / 0) 100%)",
          }}
        />
      </div>

      <Container>
        <div
          className="grid max-w-160 gap-6 pb-14"
          style={{ paddingTop: "clamp(40px, 8vw, 96px)" }}
        >
          <div className="flex items-center gap-2.5">{eyebrow}</div>
          {/* min-w-0 + overflow-wrap:anywhere so a raw release name with no
            spaces (e.g. "Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv") wraps
            instead of overflowing the hero to the right — grid items default
            to min-width:auto, which otherwise lets the giant unbreakable
            token blow past the container. */}
          {titlePending ? (
            <div
              className="h-[0.9em] w-[min(70%,28rem)] animate-pulse rounded-lg bg-muted/40"
              style={{ height: "clamp(44px, 9vw, 88px)" }}
              aria-hidden
            />
          ) : (
            <h1
              className="display min-w-0 text-foreground [overflow-wrap:anywhere]"
              style={{ fontSize: "clamp(44px, 9vw, 88px)" }}
            >
              {title}
            </h1>
          )}
          {meta.length > 0 && (
            <div className="flex flex-wrap items-center gap-3.5 text-[13.5px] text-muted-foreground">
              {meta.map((m, i) => (
                <span key={m} className="flex items-center gap-3.5">
                  {i > 0 && <span className="size-0.75 rounded-full bg-fg-dim" />}
                  {m}
                </span>
              ))}
            </div>
          )}
          {overview && (
            <p
              className="max-w-[60ch] leading-relaxed text-muted-foreground"
              style={{ fontSize: "clamp(15px, 1.6vw, 17px)", textWrap: "pretty" }}
            >
              {overview}
            </p>
          )}
          <div className="mt-1 flex flex-wrap gap-2.5">{actions}</div>
          {footer}
        </div>
      </Container>
    </section>
  );
}

function ResumeHero({ item }: { item: ContinueWatchingItem }) {
  // Only pull TMDB art/overview once the server has verified the match —
  // a wrong backdrop/title on the giant hero is worse than the bare name.
  const metaQ = useQuery({
    queryKey: ["tmdb", item.tmdb_id, item.kind],
    queryFn: () => metadata.tmdb(item.tmdb_id!, item.kind ?? undefined),
    enabled: item.tmdb_id != null && item.tmdb_verified,
    staleTime: 5 * 60_000,
  });
  const md = metaQ.data;
  const title = md?.title ?? item.torrent_name;
  const remaining =
    item.duration_seconds && item.duration_seconds > 0
      ? Math.max(0, item.duration_seconds - item.position_seconds)
      : 0;
  const progress =
    item.duration_seconds && item.duration_seconds > 0
      ? Math.min(1, item.position_seconds / item.duration_seconds)
      : 0;
  const meta = [
    md?.year ? String(md.year) : null,
    item.kind === "tv" ? "Series" : "Movie",
    md?.number_of_seasons ? `${md.number_of_seasons} seasons` : null,
  ].filter((x): x is string => Boolean(x));

  return (
    <HeroLayout
      eyebrow={
        <>
          <span className="inline-flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-[0.08em] text-primary">
            <Sparkles className="size-3.5" />
            Continue tonight
          </span>
          <span className="eyebrow">· Resume</span>
        </>
      }
      backdropUrl={tmdbImage(md?.backdrop_path, "original")}
      title={title}
      titlePending={metaQ.isLoading}
      meta={meta}
      overview={md?.overview}
      actions={
        <Button asChild size="lg" className="h-11">
          <Link to={`/watch/${item.infohash}/${item.file_idx}`}>
            <Play className="size-4.5" />
            Resume
          </Link>
        </Button>
      }
      footer={
        progress > 0 ? (
          <div className="mt-2 flex items-center gap-3.5 text-[13px] text-fg-dim">
            <div className="h-0.75 w-50 max-w-[60vw] overflow-hidden rounded-full bg-elev-2">
              <div
                className="h-full rounded-full bg-primary"
                style={{ width: `${progress * 100}%` }}
              />
            </div>
            {remaining > 0 && <span>{fmtLeft(remaining)}</span>}
          </div>
        ) : undefined
      }
    />
  );
}

function FeaturedHero({ result }: { result: SearchResult }) {
  const [previewOpen, setPreviewOpen] = useState(false);
  const metaQ = useQuery({
    queryKey: ["tmdb", result.tmdb_id, result.kind],
    queryFn: () => metadata.tmdb(result.tmdb_id!, result.kind ?? undefined),
    enabled: result.tmdb_id != null,
    staleTime: 5 * 60_000,
  });
  const md = metaQ.data;
  const meta = [
    result.year ? String(result.year) : null,
    result.kind === "tv" ? "Series" : "Movie",
    result.seeders != null ? `${result.seeders} seeders` : null,
  ].filter((x): x is string => Boolean(x));

  return (
    <>
      <HeroLayout
        eyebrow={
          <span className="inline-flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-[0.08em] text-primary">
            <Sparkles className="size-3.5" />
            Featured
          </span>
        }
        backdropUrl={tmdbImage(md?.backdrop_path, "original")}
        // Prefer the clean TMDB title; otherwise tidy the raw SCENE name.
        // We can't gate on `tmdb_verified` here (featured carries the
        // indexer's unverified, often-null tmdb_id), so the fallback is a
        // best-effort prettified release name rather than a guaranteed
        // clean title.
        title={md?.title ?? prettySceneName(result.title)}
        titlePending={metaQ.isLoading}
        meta={meta}
        overview={md?.overview}
        actions={
          <Button size="lg" className="h-11" onClick={() => setPreviewOpen(true)}>
            <ArrowUpRight className="size-4.5" />
            View release
          </Button>
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

function LibraryHero({ torrent }: { torrent: TorrentView }) {
  // Only trust TMDB art/title once the server has verified the match —
  // same discipline as ResumeHero (a wrong giant backdrop is worse than
  // the bare name).
  const metaQ = useQuery({
    queryKey: ["tmdb", torrent.tmdb_id, torrent.kind],
    queryFn: () => metadata.tmdb(torrent.tmdb_id!, torrent.kind ?? undefined),
    enabled: torrent.tmdb_id != null && torrent.tmdb_verified,
    staleTime: 5 * 60_000,
  });
  const md = metaQ.data;
  const title = md?.title
    ? md.title
    : torrent.name
      ? prettySceneName(torrent.name)
      : torrent.infohash.slice(0, 12);

  // Mirror LibraryCard's landing logic: collection page if grouped,
  // else play the largest video, else the library grid.
  const videos = torrent.files.filter((f) => VIDEO_RE.test(f.path));
  const largestVideo =
    videos.length > 0 ? videos.reduce((a, b) => (b.size_bytes > a.size_bytes ? b : a)) : null;
  const href = torrent.collection_id
    ? `/collection/${torrent.collection_id}`
    : largestVideo
      ? `/watch/${torrent.infohash}/${largestVideo.index}`
      : "/library";

  const meta = [
    md?.year ? String(md.year) : null,
    torrent.kind === "tv" ? "Series" : "Movie",
    formatSize(torrent.total_size_bytes),
  ].filter((x): x is string => Boolean(x));

  return (
    <HeroLayout
      eyebrow={
        <span className="inline-flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-[0.08em] text-primary">
          <Sparkles className="size-3.5" />
          In your library
        </span>
      }
      backdropUrl={tmdbImage(md?.backdrop_path, "original")}
      title={title}
      titlePending={metaQ.isLoading}
      meta={meta}
      overview={md?.overview}
      actions={
        <Button asChild size="lg" className="h-11">
          <Link to={href}>
            <Play className="size-4.5" />
            {torrent.collection_id ? "Open" : "Play"}
          </Link>
        </Button>
      }
    />
  );
}

function fmtLeft(seconds: number): string {
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${m.toString().padStart(2, "0")}m left`;
  return `${m}:${s.toString().padStart(2, "0")} left`;
}

// ── CARDS ────────────────────────────────────────────────────────────────────

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
          <Tag variant="accent">{item.new_count} new</Tag>
        ) : (
          <Bookmark className="size-3.5 text-white/85 drop-shadow" />
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
            <Bookmark className="size-3.5 text-white/85 drop-shadow" />
          ) : result.freeleech ? (
            <Tag variant="success" upper>
              FL
            </Tag>
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
  // Best landing target:
  //  1. its collection page when the torrent is grouped into one (season
  //     packs / multi-episode releases → the episode list);
  //  2. else /watch the largest video (single-file, or just start playing —
  //     the WatchPage lists the torrent's other files anyway);
  //  3. else the generic library grid (no playable video at all).
  const largestVideo =
    videos.length > 0 ? videos.reduce((a, b) => (b.size_bytes > a.size_bytes ? b : a)) : null;
  const href = torrent.collection_id
    ? `/collection/${torrent.collection_id}`
    : largestVideo
      ? `/watch/${torrent.infohash}/${largestVideo.index}`
      : "/library";
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
