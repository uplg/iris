import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { getRouteApi, Link } from "@tanstack/react-router";
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

import { Container } from "@/components/Container";
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
 * Release name for display in the torrents list. A single-file torrent is
 * named after its only file (".mkv" and all), so the raw `name` reads like
 * a path; stripping the trailing video extension turns
 * `Midnight.2021.MULTi.1080p.WEB.x264-FW.mkv` into the release name
 * `Midnight.2021.MULTi.1080p.WEB.x264-FW`. Multi-file release / folder
 * names carry no extension and pass through unchanged.
 */
const releaseName = (name: string): string => name.replace(VIDEO_RE, "");

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
const libraryRoute = getRouteApi("/auth/shell/library");

export function LibraryPage() {
  const { view: viewParam } = libraryRoute.useSearch();
  const navigate = libraryRoute.useNavigate();
  const view = viewParam === "torrents" ? "torrents" : "collections";

  // Read-only stats off the (already-cached on Home) torrents list, for
  // the header stat cards. Cheap — shared query key.
  const statsQ = useQuery({ queryKey: ["torrents"], queryFn: torrents.list, staleTime: 5_000 });
  const stats = useMemo(() => {
    const list = statsQ.data ?? [];
    return {
      count: list.length,
      size: list.reduce((s, t) => s + (t.total_size_bytes ?? 0), 0),
      downloading: list.filter((t) => t.progress_pct < 99.9).length,
    };
  }, [statsQ.data]);

  return (
    <Container>
      <div className="grid gap-7">
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div className="grid gap-1.5">
            <span className="eyebrow">On disk</span>
            <h1 className="display" style={{ fontSize: "clamp(36px, 5vw, 56px)" }}>
              Library
            </h1>
          </div>
          <div className="flex flex-wrap gap-3">
            <Stat label="Items" value={stats.count.toLocaleString()} />
            <Stat label="Storage" value={formatSize(stats.size)} />
            <Stat label="Downloading" value={String(stats.downloading)} sub="in flight" accent />
          </div>
        </header>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            {view === "collections"
              ? "Movies and series grouped together."
              : "Every torrent (raw view)."}
          </p>
          <div className="flex items-center gap-1 rounded-[10px] border border-border bg-elev p-1">
            <ViewToggleButton
              active={view === "collections"}
              onClick={() => navigate({ search: {}, replace: true })}
              icon={<LayoutGrid className="size-3.5" />}
              label="Collections"
            />
            <ViewToggleButton
              active={view === "torrents"}
              onClick={() => navigate({ search: { view: "torrents" }, replace: true })}
              icon={<List className="size-3.5" />}
              label="Torrents"
            />
          </div>
        </div>

        {view === "collections" ? <CollectionsView /> : <TorrentsView />}
      </div>
    </Container>
  );
}

function Stat({
  label,
  value,
  sub,
  accent,
}: {
  label: string;
  value: string;
  sub?: string;
  accent?: boolean;
}) {
  return (
    <div className="grid min-w-[120px] gap-0.5 rounded-[10px] border border-border bg-surface px-4 py-3">
      <span className="eyebrow">{label}</span>
      <div className="flex items-baseline gap-1.5">
        <span className={cn("display text-[22px]", accent ? "text-primary" : "text-foreground")}>
          {value}
        </span>
        {sub && <span className="text-[11.5px] text-fg-dim">{sub}</span>}
      </div>
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
        "flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium transition",
        active
          ? "bg-brand-soft text-primary"
          : "text-muted-foreground hover:bg-accent hover:text-foreground",
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

type SortMode = "alpha" | "recent" | "size";

const SORT_LABEL: Record<SortMode, string> = {
  alpha: "Alphabetical",
  recent: "Recently added",
  size: "Total size",
};

function CollectionsView() {
  const navigate = libraryRoute.useNavigate();
  const { kind: kindFilter, sort: sortParam } = libraryRoute.useSearch();
  const { data, isLoading, error } = useQuery({
    queryKey: ["library", "collections"],
    queryFn: () => library.list("collections"),
    refetchInterval: 5_000,
  });

  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  // URL-persisted: kind=movie|tv (omit = all). The updater preserves the
  // other library filters (e.g. sort) rather than clobbering them.
  const setKindFilter = (next: "movie" | "tv" | null) => {
    navigate({ search: (prev) => ({ ...prev, kind: next ?? undefined }), replace: true });
  };
  // Sort persisted in URL too (`?sort=alpha|recent|size`) so a refresh
  // keeps the user's choice. Defaults to `recent` (= the previous
  // server-side ordering), which we encode as the absent param.
  const sort = sortParam ?? "recent";
  const setSort = (next: SortMode) => {
    navigate({
      search: (prev) => ({ ...prev, sort: next === "recent" ? undefined : next }),
      replace: true,
    });
  };

  const allItems = useMemo<CollectionListItem[]>(
    () => (data && data.view === "collections" ? data.items : []),
    [data],
  );

  const items = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    let out = allItems.filter((c) => {
      if (kindFilter === "movie" && c.kind !== "movie") return false;
      if (kindFilter === "tv" && c.kind !== "tv") return false;
      if (!q) return true;
      return (
        c.display_title.toLowerCase().includes(q) ||
        (c.tmdb_id != null && String(c.tmdb_id).includes(q))
      );
    });
    out = [...out];
    switch (sort) {
      case "alpha":
        out.sort((a, b) => a.display_title.localeCompare(b.display_title));
        break;
      case "size":
        out.sort((a, b) => b.total_size_bytes - a.total_size_bytes);
        break;
      case "recent":
        // Server already orders by recent — keep the data order.
        break;
    }
    return out;
  }, [allItems, deferredSearch, kindFilter, sort]);

  if (isLoading) {
    return <SkeletonCard count={5} />;
  }
  if (error) {
    return <ErrorState error={error} />;
  }
  if (allItems.length === 0) {
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
    <div className="grid gap-3">
      <CollectionsFilters
        search={search}
        onSearchChange={setSearch}
        kindFilter={kindFilter}
        onKindChange={setKindFilter}
        sort={sort}
        onSortChange={setSort}
        total={allItems.length}
        shown={items.length}
      />
      {items.length === 0 ? (
        <EmptyState title="No matches" body="Adjust the search or filters to see more." />
      ) : (
        <VirtualCollectionsGrid items={items} onPick={(c) => routeCollection(c, navigate)} />
      )}
    </div>
  );
}

function CollectionsFilters({
  search,
  onSearchChange,
  kindFilter,
  onKindChange,
  sort,
  onSortChange,
  total,
  shown,
}: {
  search: string;
  onSearchChange: (v: string) => void;
  kindFilter: "movie" | "tv" | undefined;
  onKindChange: (k: "movie" | "tv" | null) => void;
  sort: SortMode;
  onSortChange: (s: SortMode) => void;
  total: number;
  shown: number;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="relative grow basis-full sm:min-w-56 sm:basis-0">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <input
          type="text"
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="Search collections…"
          className="w-full rounded-md border border-border bg-card/40 py-1.5 pl-9 pr-9 text-sm placeholder:text-muted-foreground focus:border-primary/40 focus:outline-none focus:ring-2 focus:ring-primary/20"
        />
        {search && (
          <button
            type="button"
            onClick={() => onSearchChange("")}
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-muted-foreground hover:bg-muted/40 hover:text-foreground"
          >
            <X className="size-3" />
            <span className="sr-only">Clear search</span>
          </button>
        )}
      </div>
      <div className="flex items-center gap-1 rounded-md border border-border bg-card/40 p-0.5 text-xs">
        <KindChip active={kindFilter == null} onClick={() => onKindChange(null)}>
          All
        </KindChip>
        <KindChip
          active={kindFilter === "movie"}
          onClick={() => onKindChange("movie")}
          icon={<Film className="size-3" />}
        >
          Movies
        </KindChip>
        <KindChip
          active={kindFilter === "tv"}
          onClick={() => onKindChange("tv")}
          icon={<Tv className="size-3" />}
        >
          Series
        </KindChip>
      </div>
      <select
        value={sort}
        onChange={(e) => onSortChange(e.target.value as SortMode)}
        className="rounded-md border border-border bg-card/40 px-2 py-1.5 text-xs focus:border-primary/40 focus:outline-none focus:ring-2 focus:ring-primary/20"
      >
        {(Object.keys(SORT_LABEL) as SortMode[]).map((m) => (
          <option key={m} value={m}>
            {SORT_LABEL[m]}
          </option>
        ))}
      </select>
      <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">
        {shown === total ? `${total} collections` : `${shown} / ${total}`}
      </span>
    </div>
  );
}

function KindChip({
  active,
  onClick,
  icon,
  children,
}: {
  active: boolean;
  onClick: () => void;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1 rounded px-2.5 py-1 transition",
        active
          ? "bg-primary/15 text-primary"
          : "text-muted-foreground hover:bg-muted/40 hover:text-foreground",
      )}
    >
      {icon}
      {children}
    </button>
  );
}

/**
 * Virtualized grid of collection cards. Adapts the lane count to the
 * container width (`ResizeObserver` watches the parent), so the same
 * component switches between 2/3/4/5 columns as breakpoints would.
 * Each virtual row renders `lanes` cards side-by-side; only the rows
 * intersecting the viewport are kept in the DOM.
 */
function VirtualCollectionsGrid({
  items,
  onPick,
}: {
  items: CollectionListItem[];
  onPick: (c: CollectionListItem) => void;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const qc = useQueryClient();
  // Hide a ghost card for THIS user only. Non-destructive (History and
  // playback rows stay); watching the show again resurfaces it.
  const dismissGhost = useMutation({
    mutationFn: (c: CollectionListItem) => me.dismissGone({ collection_id: c.id }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["library"] }),
  });
  // Lane count + row height are derived from container width via the
  // CSS variable trick: the wrapper sets `--lanes` based on its width,
  // and we read it back through ResizeObserver. Avoids the breakpoint-
  // duplication trap (Tailwind classes vs JS thresholds).
  const [lanes, setLanes] = useState(5);
  useResizeObserver(parentRef, (width) => {
    // Same intent as `sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5`:
    //   < 640  → 2 lanes
    //   < 1024 → 3 lanes
    //   < 1280 → 4 lanes
    //   else   → 5 lanes
    const next = width < 640 ? 2 : width < 1024 ? 3 : width < 1280 ? 4 : 5;
    setLanes(next);
  });

  const rowCount = Math.ceil(items.length / lanes);
  // Aspect-2/3 poster + title + subtitle + a touch of padding/badge —
  // empirically ~280-320 px depending on lane width. Slightly over-
  // estimated so initial scroll feels smooth; `measureElement` fixes
  // up the real height per row once rendered.
  const estimatedRowHeight = 320;

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => estimatedRowHeight,
    overscan: 2,
  });

  return (
    <div ref={parentRef} className="max-h-[calc(100vh-14rem)] overflow-y-auto rounded-lg">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((vrow) => {
          const start = vrow.index * lanes;
          const slice = items.slice(start, start + lanes);
          return (
            <div
              key={vrow.key}
              ref={virtualizer.measureElement}
              data-index={vrow.index}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                right: 0,
                transform: `translateY(${vrow.start}px)`,
              }}
              className="px-1 pb-4"
            >
              <div
                className="grid gap-4"
                style={{
                  gridTemplateColumns: `repeat(${lanes}, minmax(0, 1fr))`,
                }}
              >
                {slice.map((c) => (
                  <div key={c.id} className="group relative">
                    <MediaCard
                      title={c.display_title}
                      subtitle={collectionSubtitle(c)}
                      // tmdb_id on the collection is now derived server-
                      // side from the SCENE-cleaned name (see
                      // `tmdb_resolve` + the ingestion override).
                      // Trustworthy enough to drive poster lookups —
                      // when missing we fall back to the kind-aware
                      // placeholder.
                      tmdbId={c.tmdb_id}
                      kind={c.kind}
                      onClick={() => onPick(c)}
                      badge={collectionBadge(c)}
                      // Ghost = every torrent reclaimed, but the caller
                      // watched this — kept in place, greyed. Clicking
                      // only NAVIGATES to the collection page; any
                      // re-download stays a deliberate user action there.
                      className={cn(c.ghost && "opacity-55 grayscale")}
                    />
                    {/* Ghost cards get a hover X: hide from MY library
                        (per-user, non-destructive — same pattern as the
                        Continue Watching manage button). */}
                    {c.ghost && (
                      <button
                        type="button"
                        aria-label="Hide from my library"
                        title="Hide from my library (your history is kept)"
                        disabled={dismissGhost.isPending}
                        onClick={(e) => {
                          e.stopPropagation();
                          dismissGhost.mutate(c);
                        }}
                        className="focus-ring absolute top-2 left-2 grid size-8 place-items-center rounded-full bg-black/60 text-white opacity-0 backdrop-blur transition-opacity group-hover:opacity-100 focus-visible:opacity-100 disabled:opacity-60"
                      >
                        <X className="size-4" />
                      </button>
                    )}
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Watch a `ref`-ed element's `clientWidth`. The hook batches changes
 * via `ResizeObserver` (browser-native — not a setTimeout poll, so it
 * stays compliant with the web/ timer rule) and only fires the
 * callback when the integer width actually changes, avoiding a re-
 * render storm during continuous resize.
 */
function useResizeObserver(
  ref: React.RefObject<HTMLElement | null>,
  onChange: (width: number) => void,
) {
  const lastWidth = useRef(0);
  const cb = useRef(onChange);
  cb.current = onChange;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const w = Math.floor(entries[0]?.contentRect.width ?? 0);
      if (w !== lastWidth.current && w > 0) {
        lastWidth.current = w;
        cb.current(w);
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
}

function collectionSubtitle(c: CollectionListItem): string {
  if (c.ghost) return "No longer on disk";
  const parts: string[] = [];
  if (c.kind === "tv" && c.episode_count > 0) {
    parts.push(`${c.episode_count} ep`);
  } else if (c.kind === "tv" && c.torrent_count > 1) {
    parts.push(`${c.torrent_count} torrents`);
  }
  parts.push(formatSize(c.total_size_bytes));
  return parts.join(" · ");
}

function collectionBadge(c: CollectionListItem): React.ReactNode {
  if (c.ghost) {
    return (
      <Badge variant="outline" className="bg-background/70 text-[10px] shadow-md">
        Gone
      </Badge>
    );
  }
  if (c.kind === "tv" && c.episode_count > 0) {
    return (
      <Badge variant="secondary" className="text-[10px] shadow-md">
        {c.episode_count} ep
      </Badge>
    );
  }
  return undefined;
}

function routeCollection(
  c: CollectionListItem,
  navigate: ReturnType<typeof libraryRoute.useNavigate>,
) {
  // Always land on the collection page. The /series/:tmdb_id route
  // is the Watchlist surface (TMDB-driven episode grid) and only
  // makes sense for shows the user has explicitly followed — when
  // we tried to use it from the library we kept landing users on a
  // "broken follow" view whenever the indexer-attached tmdb_id was
  // wrong. CollectionPage shows the actual SCENE-grouped content
  // we have on disk, which is always correct.
  navigate({ to: "/collection/$id", params: { id: String(c.id) } });
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

  const totalUploaded = data && data.view === "torrents" ? data.total_uploaded_bytes : 0;

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
        removingInfohash={remove.isPending ? (remove.variables ?? null) : null}
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
    <div ref={parentRef} className="max-h-[calc(100vh-18rem)] overflow-y-auto rounded-lg">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
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

function SeedSummary({ totalUploaded, items }: { totalUploaded: number; items: TorrentView[] }) {
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
          <span className="text-xs text-emerald-300/80 tabular-nums">ratio {ratio.toFixed(2)}</span>
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
          tmdbId={t.tmdb_id ?? null}
          kind={t.kind ?? null}
          verified={t.tmdb_verified}
        />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <h3 className="truncate text-sm font-medium leading-snug" title={t.name ?? undefined}>
                {t.name ? releaseName(t.name) : t.infohash}
              </h3>
              <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[10px] text-muted-foreground">
                <StateBadge state={t.state} />
                <HealthBadge peers={t.peers} finished={finished} state={t.state} />
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
                  <Link
                    to="/watch/$infohash/$idx"
                    params={{ infohash: t.infohash, idx: String(videos[0]!.index) }}
                  >
                    <Play className="size-3.5" />
                    Play
                  </Link>
                </Button>
              )}
              {videos.length > 1 && (
                <Button size="sm" variant="outline" onClick={() => setExpanded((v) => !v)}>
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
                <span className="text-foreground">{formatSize(t.total_size_bytes)}</span>
              </span>
              <span className="text-muted-foreground">
                <span className="text-sky-300">↓</span> {formatSize(t.download_speed_bps)}/s
                <span className="mx-1.5">·</span>
                <span className="text-emerald-300">↑</span> {formatSize(t.upload_speed_bps)}/s
                <span className="mx-1.5">·</span>
                {t.peers} peers
              </span>
            </div>
            <Progress value={pct} className={cn("h-1.5", finished && "[&>*]:bg-emerald-500/70")} />
          </div>

          <p className="text-[10px] text-muted-foreground">
            Added by <span className="text-foreground/90">{t.added_by_name}</span>
            {" · "}
            {new Date(t.added_at).toLocaleDateString()}
          </p>

          {t.error && <p className="text-xs text-destructive">{t.error}</p>}
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
  // Resolve by the *served* tmdb_id. The backend now sets a torrent's
  // `tmdb_id` to its parent collection's resolved id
  // (`effective_tmdb_id`) — the same stable source the collection grid,
  // Home shelf and collection page use — so this view converges on it
  // too (a c411 "Saison N" pack used to resolve by its useless name
  // here and showed a wrong thumb). Shared `["tmdb", …]` query key
  // dedupes against `MediaCard` for the same id.
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
          className={cn("h-full w-full object-cover", !verified && "opacity-70")}
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
            <span className="text-emerald-300">{watchedPct.toFixed(0)}% watched</span>
          ) : null}
        </div>
        {!watch?.completed && watchedPct != null && (
          <Progress className="mt-1 h-0.5" value={watchedPct} />
        )}
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <Button asChild size="sm">
          <Link to="/watch/$infohash/$idx" params={{ infohash, idx: String(file.index) }}>
            <Play className="size-3.5" />
            {watchedPct != null && watchedPct > 0 && !watch?.completed ? "Resume" : "Play"}
          </Link>
        </Button>
        <Button asChild size="sm" variant="outline" title="Download">
          <a href={torrents.downloadUrl(infohash, file.index)} download={fname}>
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
    <Badge variant="outline" className={`text-[10px] uppercase ${styles[state]}`}>
      {state}
    </Badge>
  );
}
