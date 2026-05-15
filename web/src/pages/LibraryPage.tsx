import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useMemo, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Download,
  Film,
  LayoutGrid,
  List,
  Play,
  Search,
  Trash2,
  Tv,
  Users,
  X,
} from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { EmptyState, ErrorState, SkeletonCard } from "@/components/State";
import {
  library,
  me,
  metadata,
  tmdbImage,
  torrents,
  type CollectionListItem,
  type ContinueWatchingItem,
  type MediaKind,
  type TorrentView,
} from "@/lib/api";
import { formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/**
 * Library page with two views:
 *
 *   * Collections (default): Netflix-style poster grid, one card per
 *     logical entity. Click a TV show → its Series page; click a movie
 *     → straight to /watch.
 *   * Torrents: the legacy flat list — kept for power users / debug,
 *     reachable via the toggle in the top-right.
 *
 * The toggle is mirrored into `?view=torrents` so a refresh keeps the
 * user's pick.
 */
export function LibraryPage() {
  const [params, setParams] = useSearchParams();
  const view = params.get("view") === "torrents" ? "torrents" : "collections";

  return (
    <div className="grid gap-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">Library</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {view === "collections"
              ? "Movies and series grouped together."
              : "Every torrent (raw view)."}
          </p>
        </div>
        <div className="flex items-center gap-1 rounded-md border border-border bg-card/40 p-0.5">
          <ViewToggleButton
            active={view === "collections"}
            onClick={() => setParams({}, { replace: true })}
            icon={<LayoutGrid className="size-3.5" />}
            label="Collections"
          />
          <ViewToggleButton
            active={view === "torrents"}
            onClick={() => setParams({ view: "torrents" }, { replace: true })}
            icon={<List className="size-3.5" />}
            label="Torrents"
          />
        </div>
      </header>

      {view === "collections" ? <CollectionsView /> : <TorrentsView />}
    </div>
  );
}

function ViewToggleButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded px-3 py-1 text-xs transition",
        active
          ? "bg-primary/15 text-primary"
          : "text-muted-foreground hover:bg-muted/40 hover:text-foreground",
      )}
    >
      {icon}
      {label}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Collections view
// ---------------------------------------------------------------------------

function CollectionsView() {
  const navigate = useNavigate();
  const { data, isLoading, error } = useQuery({
    queryKey: ["library", "collections"],
    queryFn: () => library.list("collections"),
    refetchInterval: 5_000,
  });

  if (isLoading) {
    return <SkeletonCard count={5} />;
  }
  if (error) {
    return <ErrorState error={error} />;
  }
  const items: CollectionListItem[] =
    data && data.view === "collections" ? data.items : [];
  if (items.length === 0) {
    return (
      <EmptyState
        title="Library is empty"
        body={
          <>
            Start a{" "}
            <Link to="/search" className="underline">
              search
            </Link>{" "}
            to add your first title.
          </>
        }
      />
    );
  }
  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
      {items.map((c) => (
        <MediaCard
          key={c.id}
          title={c.display_title}
          subtitle={collectionSubtitle(c)}
          // tmdb_id on the collection is now derived server-side from
          // the SCENE-cleaned name (see `tmdb_resolve` + the ingestion
          // override). Trustworthy enough to drive poster lookups —
          // when missing we fall back to the kind-aware placeholder.
          tmdbId={c.tmdb_id}
          kind={c.kind}
          onClick={() => routeCollection(c, navigate)}
          badge={
            c.kind === "tv" && c.episode_count > 0 ? (
              <Badge variant="secondary" className="text-[10px] shadow-md">
                {c.episode_count} ep
              </Badge>
            ) : undefined
          }
        />
      ))}
    </div>
  );
}

function collectionSubtitle(c: CollectionListItem): string {
  const parts: string[] = [];
  if (c.kind === "tv" && c.torrent_count > 1) {
    parts.push(`${c.torrent_count} torrents`);
  }
  parts.push(formatSize(c.total_size_bytes));
  return parts.join(" · ");
}

function routeCollection(
  c: CollectionListItem,
  navigate: ReturnType<typeof useNavigate>,
) {
  // Always land on the collection page. The /series/:tmdb_id route
  // is the Watchlist surface (TMDB-driven episode grid) and only
  // makes sense for shows the user has explicitly followed — when
  // we tried to use it from the library we kept landing users on a
  // "broken follow" view whenever the indexer-attached tmdb_id was
  // wrong. CollectionPage shows the actual SCENE-grouped content
  // we have on disk, which is always correct.
  navigate(`/collection/${c.id}`);
}

// ---------------------------------------------------------------------------
// Torrents view — virtualized power-user list
// ---------------------------------------------------------------------------

function TorrentsView() {
  const qc = useQueryClient();
  const { data, isLoading, error } = useQuery({
    queryKey: ["library", "torrents"],
    queryFn: () => library.list("torrents"),
    refetchInterval: 3_000,
  });
  const cwQ = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });

  const remove = useMutation({
    mutationFn: (infohash: string) => torrents.remove(infohash),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["library"] });
      void qc.invalidateQueries({ queryKey: ["continue-watching"] });
    },
  });

  const [filter, setFilter] = useState("");
  const allItems = useMemo<TorrentView[]>(
    () => (data && data.view === "torrents" ? data.items : []),
    [data],
  );
  const items = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return allItems;
    return allItems.filter(
      (t) =>
        (t.name ?? "").toLowerCase().includes(q) ||
        t.infohash.toLowerCase().includes(q) ||
        (t.added_by_name ?? "").toLowerCase().includes(q),
    );
  }, [allItems, filter]);

  const totalUploaded =
    data && data.view === "torrents" ? data.total_uploaded_bytes : 0;

  if (isLoading) return <SkeletonCard count={3} />;
  if (error) return <ErrorState error={error} />;
  if (allItems.length === 0)
    return <EmptyState title="No torrents" body="Library is empty for now." />;

  return (
    <div className="grid gap-3">
      <SeedSummary totalUploaded={totalUploaded} items={allItems} />
      <TorrentFilter
        value={filter}
        onChange={setFilter}
        total={allItems.length}
        shown={items.length}
      />
      <VirtualTorrentList
        items={items}
        progress={cwQ.data ?? []}
        onRemove={(infohash) => remove.mutate(infohash)}
        removingInfohash={remove.isPending ? remove.variables ?? null : null}
      />
    </div>
  );
}

function TorrentFilter({
  value,
  onChange,
  total,
  shown,
}: {
  value: string;
  onChange: (v: string) => void;
  total: number;
  shown: number;
}) {
  return (
    <div className="flex items-center gap-2">
      <div className="relative flex-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="Filter by name, infohash, uploader…"
          className="w-full rounded-md border border-border bg-card/40 py-1.5 pl-9 pr-9 text-sm placeholder:text-muted-foreground focus:border-primary/40 focus:outline-none focus:ring-2 focus:ring-primary/20"
        />
        {value && (
          <button
            type="button"
            onClick={() => onChange("")}
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-muted-foreground hover:bg-muted/40 hover:text-foreground"
          >
            <X className="size-3" />
            <span className="sr-only">Clear filter</span>
          </button>
        )}
      </div>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
        {shown === total ? `${total} torrents` : `${shown} / ${total}`}
      </span>
    </div>
  );
}

/**
 * Virtualized scroller. Rows have dynamic heights (expandable file
 * lists) — `measureElement` + ResizeObserver auto-updates the
 * virtualizer's size cache as users expand / collapse. The outer
 * scroll container is bounded to a sensible viewport height so
 * the page itself doesn't grow unbounded; large libraries stay
 * inside that scroller.
 */
function VirtualTorrentList({
  items,
  progress,
  onRemove,
  removingInfohash,
}: {
  items: TorrentView[];
  progress: ContinueWatchingItem[];
  onRemove: (infohash: string) => void;
  removingInfohash: string | null;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 140,
    overscan: 4,
    getItemKey: (i) => items[i]!.infohash,
  });

  // Group continue-watching items by infohash once per render so each
  // row can do an O(1) lookup. With N rows × M progress rows we'd
  // otherwise be O(N·M) on every refetch tick.
  const progressByInfohash = useMemo(() => {
    const map = new Map<string, Map<number, ContinueWatchingItem>>();
    for (const p of progress) {
      let bucket = map.get(p.infohash);
      if (!bucket) {
        bucket = new Map();
        map.set(p.infohash, bucket);
      }
      bucket.set(p.file_idx, p);
    }
    return map;
  }, [progress]);

  return (
    <div
      ref={parentRef}
      className="max-h-[calc(100vh-18rem)] overflow-y-auto rounded-lg"
    >
      <div
        style={{ height: virtualizer.getTotalSize(), position: "relative" }}
      >
        {virtualizer.getVirtualItems().map((v) => {
          const t = items[v.index]!;
          return (
            <div
              key={v.key}
              ref={virtualizer.measureElement}
              data-index={v.index}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                right: 0,
                transform: `translateY(${v.start}px)`,
              }}
              className="pb-3 pr-1"
            >
              <TorrentRow
                t={t}
                progressByFile={progressByInfohash.get(t.infohash)}
                onRemove={() => onRemove(t.infohash)}
                removing={removingInfohash === t.infohash}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SeedSummary({
  totalUploaded,
  items,
}: {
  totalUploaded: number;
  items: TorrentView[];
}) {
  const liveUpSpeed = items.reduce((s, t) => s + t.upload_speed_bps, 0);
  const liveDownSpeed = items.reduce((s, t) => s + t.download_speed_bps, 0);
  const downloaded = items.reduce((s, t) => s + t.progress_bytes, 0);
  const ratio = downloaded > 0 ? totalUploaded / downloaded : null;
  return (
    <div className="sticky top-2 z-10 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-emerald-500/20 bg-emerald-950/40 px-4 py-3 backdrop-blur">
      <div className="flex items-baseline gap-2">
        <span className="text-[10px] uppercase tracking-wider text-emerald-300/70">
          Seeded all-time
        </span>
        <span className="text-lg font-semibold tabular-nums text-emerald-100">
          {formatSize(totalUploaded)}
        </span>
        {ratio != null && (
          <span className="text-xs text-emerald-300/80 tabular-nums">
            ratio {ratio.toFixed(2)}
          </span>
        )}
      </div>
      <div className="flex items-center gap-4 text-xs tabular-nums text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <span className="text-emerald-300">↑</span>
          {formatSize(liveUpSpeed)}/s
        </span>
        <span className="inline-flex items-center gap-1">
          <span className="text-sky-300">↓</span>
          {formatSize(liveDownSpeed)}/s
        </span>
        <span>{items.length} active</span>
      </div>
    </div>
  );
}

function TorrentRow({
  t,
  progressByFile,
  onRemove,
  removing,
}: {
  t: TorrentView;
  progressByFile: Map<number, ContinueWatchingItem> | undefined;
  onRemove: () => void;
  removing: boolean;
}) {
  const pct = Math.min(100, Math.max(0, t.progress_pct));
  const videos = t.files.filter((f) => VIDEO_RE.test(f.path));
  const [expanded, setExpanded] = useState(false);
  const finished = t.finished || pct >= 100;
  const ratio = t.progress_bytes > 0 ? t.uploaded_bytes_total / t.progress_bytes : null;

  return (
    <div className="group rounded-lg border border-border/70 bg-card/60 transition hover:border-border">
      <div className="flex gap-4 p-4">
        <TorrentPoster
          tmdbId={t.tmdb_id}
          kind={t.kind}
          verified={t.tmdb_verified}
        />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <h3
                className="truncate text-sm font-medium leading-snug"
                title={t.name ?? undefined}
              >
                {t.name ?? t.infohash}
              </h3>
              <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[10px] text-muted-foreground">
                <StateBadge state={t.state} />
                <HealthBadge
                  peers={t.peers}
                  finished={finished}
                  state={t.state}
                />
                {ratio != null && (
                  <Badge
                    variant="outline"
                    className={cn(
                      "text-[10px] tabular-nums",
                      ratio >= 1
                        ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-200"
                        : "border-amber-500/30 bg-amber-500/5 text-amber-200/80",
                    )}
                  >
                    ratio {ratio.toFixed(2)}
                  </Badge>
                )}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              {videos.length === 1 && (
                <Button asChild size="sm" variant="outline">
                  <Link to={`/watch/${t.infohash}/${videos[0]!.index}`}>
                    <Play className="size-3.5" />
                    Play
                  </Link>
                </Button>
              )}
              {videos.length > 1 && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setExpanded((v) => !v)}
                >
                  {expanded ? (
                    <ChevronUp className="size-3.5" />
                  ) : (
                    <ChevronDown className="size-3.5" />
                  )}
                  {expanded ? "Hide" : `${videos.length} files`}
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                onClick={onRemove}
                disabled={removing}
                title="Remove torrent"
                className="text-muted-foreground hover:text-destructive"
              >
                <Trash2 className="size-3.5" />
                <span className="sr-only">Remove</span>
              </Button>
            </div>
          </div>

          {/* Progress bar with inline metrics overlay. */}
          <div className="space-y-1">
            <div className="flex items-baseline justify-between gap-2 text-[11px] tabular-nums">
              <span className="text-muted-foreground">
                {formatSize(t.progress_bytes)} /{" "}
                <span className="text-foreground">
                  {formatSize(t.total_size_bytes)}
                </span>
              </span>
              <span className="text-muted-foreground">
                <span className="text-sky-300">↓</span>{" "}
                {formatSize(t.download_speed_bps)}/s
                <span className="mx-1.5">·</span>
                <span className="text-emerald-300">↑</span>{" "}
                {formatSize(t.upload_speed_bps)}/s
                <span className="mx-1.5">·</span>
                {t.peers} peers
              </span>
            </div>
            <Progress
              value={pct}
              className={cn("h-1.5", finished && "[&>*]:bg-emerald-500/70")}
            />
          </div>

          <p className="text-[10px] text-muted-foreground">
            Added by{" "}
            <span className="text-foreground/90">{t.added_by_name}</span>
            {" · "}
            {new Date(t.added_at).toLocaleDateString()}
          </p>

          {t.error && (
            <p className="text-xs text-destructive">{t.error}</p>
          )}
        </div>
      </div>

      {expanded && videos.length > 1 && (
        <ul className="grid gap-1 border-t border-border/60 px-4 py-3 text-sm">
          {videos.map((f) => (
            <FileEntry
              key={f.index}
              file={f}
              infohash={t.infohash}
              watch={progressByFile?.get(f.index)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Small TMDB poster thumbnail. Lazy via React Query — no fetch until
 * the row is rendered, and the cache is shared with `MediaCard` so
 * re-mounts during scroll don't re-hit the network.
 */
function TorrentPoster({
  tmdbId,
  kind,
  verified,
}: {
  tmdbId: number | null;
  kind: MediaKind | null;
  verified: boolean;
}) {
  const tmdbQ = useQuery({
    queryKey: ["tmdb", tmdbId, kind],
    queryFn: () => metadata.tmdb(tmdbId!, kind ?? undefined),
    enabled: tmdbId != null,
    staleTime: 60_000,
  });
  const url = tmdbImage(tmdbQ.data?.poster_path, "w92");
  const Icon = kind === "tv" ? Tv : Film;
  return (
    <div className="relative h-20 w-14 shrink-0 overflow-hidden rounded-md border border-border bg-muted/40">
      {url ? (
        <img
          src={url}
          alt=""
          loading="lazy"
          className={cn(
            "h-full w-full object-cover",
            !verified && "opacity-70",
          )}
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center text-muted-foreground/60">
          <Icon className="size-5" />
        </div>
      )}
    </div>
  );
}

function HealthBadge({
  peers,
  finished,
  state,
}: {
  peers: number;
  finished: boolean;
  state: TorrentView["state"];
}) {
  if (state === "error" || state === "paused") return null;
  // Finished torrents seed — peer counter here is "leechers we serve".
  // Active torrents need >0 peers to make progress.
  const tone =
    peers === 0
      ? "border-rose-500/40 bg-rose-500/10 text-rose-200"
      : peers < 3
        ? "border-amber-500/40 bg-amber-500/10 text-amber-200"
        : "border-emerald-500/40 bg-emerald-500/10 text-emerald-200";
  const label = finished
    ? peers === 0
      ? "idle"
      : `${peers} leechers`
    : peers === 0
      ? "no peers"
      : `${peers} peers`;
  return (
    <Badge
      variant="outline"
      className={cn("inline-flex items-center gap-1 text-[10px] tabular-nums", tone)}
    >
      <Users className="size-2.5" />
      {label}
    </Badge>
  );
}

function FileEntry({
  file,
  infohash,
  watch,
}: {
  file: TorrentView["files"][number];
  infohash: string;
  watch: ContinueWatchingItem | undefined;
}) {
  const fname = file.path.split("/").pop() ?? file.path;
  const watchedPct =
    watch && watch.duration_seconds && watch.duration_seconds > 0
      ? Math.min(100, (watch.position_seconds / watch.duration_seconds) * 100)
      : null;
  return (
    <li className="flex items-center justify-between gap-3 rounded px-2 py-1.5 hover:bg-muted/40">
      <div className="min-w-0 flex-1">
        <div className="break-all font-mono text-xs">{file.path}</div>
        <div className="mt-0.5 flex items-center gap-3 text-[11px] text-muted-foreground">
          <span>{formatSize(file.size_bytes)}</span>
          {watch?.completed ? (
            <span className="inline-flex items-center gap-0.5 text-emerald-300">
              <CheckCircle2 className="size-3" />
              watched
            </span>
          ) : watchedPct != null ? (
            <span className="text-emerald-300">
              {watchedPct.toFixed(0)}% watched
            </span>
          ) : null}
        </div>
        {!watch?.completed && watchedPct != null && (
          <Progress className="mt-1 h-0.5" value={watchedPct} />
        )}
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <Button asChild size="sm">
          <Link to={`/watch/${infohash}/${file.index}`}>
            <Play className="size-3.5" />
            {watchedPct != null && watchedPct > 0 && !watch?.completed
              ? "Resume"
              : "Play"}
          </Link>
        </Button>
        <Button asChild size="sm" variant="outline" title="Download">
          <a
            href={torrents.downloadUrl(infohash, file.index)}
            download={fname}
          >
            <Download className="size-3.5" />
            <span className="sr-only">Download</span>
          </a>
        </Button>
      </div>
    </li>
  );
}

function StateBadge({ state }: { state: TorrentView["state"] }) {
  const styles: Record<TorrentView["state"], string> = {
    initializing: "border-sky-500/50 bg-sky-500/10 text-sky-200",
    live: "border-emerald-500/50 bg-emerald-500/10 text-emerald-200",
    paused: "border-zinc-500/50 bg-zinc-500/10 text-zinc-200",
    error: "border-rose-500/50 bg-rose-500/10 text-rose-200",
  };
  return (
    <Badge
      variant="outline"
      className={`text-[10px] uppercase ${styles[state]}`}
    >
      {state}
    </Badge>
  );
}
