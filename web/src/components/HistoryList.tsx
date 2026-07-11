import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronRight, RotateCcw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Poster } from "@/components/Poster";
import type { HistoryItem, UserHistoryItem } from "@/lib/api";
import { formatRecentTime, formatTimecode } from "@/lib/format";
import { cn } from "@/lib/utils";

type Item = HistoryItem | UserHistoryItem;

const HEADER_HEIGHT = 64;
const ROW_HEIGHT = 48;

/** One virtualized line: a collection header (poster + clean title), an
 *  episode row under it, or a standalone item (movie) that merges both. */
type VirtualRow =
  | { type: "header"; key: string; group: Group }
  | { type: "episode"; key: string; item: Item; group: Group }
  | { type: "solo"; key: string; item: Item; group: Group };

type Group = {
  key: string;
  collectionId: string | null;
  title: string;
  tmdbId: number | null;
  kind: Item["kind"];
  /** Every row's source torrent is gone — the whole collection is a
   *  "ghost": still rendered (title + poster + progress), still
   *  navigable to the collection page to re-grab. */
  ghost: boolean;
  items: Item[];
};

function itemKey(it: Item): string {
  return `${it.infohash}:${it.file_idx}`;
}

/** Group rows by collection, preserving the newest-first order of each
 *  group's most recent item (the list arrives sorted by the server). */
function buildGroups(items: Item[]): Group[] {
  const groups: Group[] = [];
  const byKey = new Map<string, Group>();
  for (const it of items) {
    const key = it.collection_id ?? `solo:${it.infohash}`;
    let g = byKey.get(key);
    if (!g) {
      g = {
        key,
        collectionId: it.collection_id ?? null,
        title: it.collection_title ?? it.torrent_name,
        tmdbId: it.tmdb_id ?? null,
        kind: it.kind,
        ghost: true,
        items: [],
      };
      byKey.set(key, g);
      groups.push(g);
    }
    g.items.push(it);
    if (!it.deleted) g.ghost = false;
  }
  return groups;
}

/** "S01E03" / "Episode 1156" / file basename — what exactly was watched. */
function episodeLabel(item: Item): string | null {
  if (item.absolute_episode != null) return `Episode ${item.absolute_episode}`;
  if (item.season != null && item.episode != null) {
    if (item.episode === 0) return `Season ${item.season}`;
    return `S${String(item.season).padStart(2, "0")}E${String(item.episode).padStart(2, "0")}`;
  }
  const base = item.file_path ? (item.file_path.split("/").pop() ?? item.file_path) : null;
  return base;
}

function statusLine(item: Item): string {
  if (item.completed) return "Watched";
  const pct =
    item.duration_seconds && item.duration_seconds > 0
      ? Math.min(100, (item.position_seconds / item.duration_seconds) * 100)
      : 0;
  return pct > 0 ? `${pct.toFixed(0)}% · ${formatTimecode(item.position_seconds)}` : "Just started";
}

function canRestore(item: Item): boolean {
  return item.deleted && item.source_provider != null && item.source_external_id != null;
}

/**
 * Watch history grouped by collection — the "ghost collections" design:
 * every show/movie the user touched stays visible under its clean title
 * + poster even after the disk-reclaim GC removed all of its torrents
 * (collections are never hard-deleted server-side, and the grouping is
 * derived from the caller's OWN history, so ghosts are per-user by
 * construction). Headers navigate to the collection page (where the
 * indexer offers allow re-grabbing); deleted episode rows offer
 * "Download again", which re-ingests the exact same release — same
 * infohash, so the stored resume position applies untouched.
 *
 * Shared by the user-facing Watch History page (`me.history()`) and the
 * admin per-user drill-down (`admin.userHistory()`); the admin view
 * passes no `onRestore`, staying read-only.
 *
 * Virtualized with per-row-type heights (same pattern as the previous
 * flat list) so it stays smooth at any history length.
 */
export function HistoryList({
  items,
  onPlay,
  onOpenCollection,
  onRestore,
  restoringKey,
}: {
  items: Item[];
  onPlay: (item: Item) => void;
  /** Navigate to `/collection/:id` — omitted in read-only (admin) usage. */
  onOpenCollection?: (collectionId: string) => void;
  /** Re-ingest a GC'd row's source release. Omitted in admin usage. */
  onRestore?: (item: Item) => void;
  /** `itemKey`-shaped id of the row currently being restored. */
  restoringKey?: string | null;
}) {
  const rows = useMemo<VirtualRow[]>(() => {
    const out: VirtualRow[] = [];
    for (const g of buildGroups(items)) {
      // A lone row with no episode coordinates (typically a movie)
      // reads better as one merged line than as header + child.
      const solo =
        g.items.length === 1 && g.items[0]!.season == null && g.items[0]!.absolute_episode == null;
      if (solo) {
        out.push({ type: "solo", key: `solo:${g.key}`, item: g.items[0]!, group: g });
        continue;
      }
      out.push({ type: "header", key: `h:${g.key}`, group: g });
      for (const it of g.items) {
        out.push({ type: "episode", key: `e:${itemKey(it)}`, item: it, group: g });
      }
    }
    return out;
  }, [items]);

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => (rows[i]!.type === "episode" ? ROW_HEIGHT : HEADER_HEIGHT),
    overscan: 10,
    getItemKey: (i) => rows[i]!.key,
  });

  return (
    <div ref={parentRef} className="max-h-[70svh] overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((v) => {
          const row = rows[v.index]!;
          return (
            <div
              key={v.key}
              data-index={v.index}
              className="absolute inset-x-0"
              style={{ top: 0, height: v.size, transform: `translateY(${v.start}px)` }}
            >
              {row.type === "header" ? (
                <GroupHeader group={row.group} onOpenCollection={onOpenCollection} />
              ) : row.type === "solo" ? (
                <SoloRow
                  item={row.item}
                  group={row.group}
                  onPlay={onPlay}
                  onOpenCollection={onOpenCollection}
                  onRestore={onRestore}
                  restoringKey={restoringKey}
                />
              ) : (
                <EpisodeRow
                  item={row.item}
                  onPlay={onPlay}
                  onRestore={onRestore}
                  restoringKey={restoringKey}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Collection header: poster + clean display title. Ghosts stay fully
 *  rendered, just greyed — the whole point is that they don't vanish. */
function GroupHeader({
  group,
  onOpenCollection,
}: {
  group: Group;
  onOpenCollection?: (collectionId: string) => void;
}) {
  const interactive = group.collectionId != null && onOpenCollection != null;
  return (
    <button
      type="button"
      disabled={!interactive}
      onClick={interactive ? () => onOpenCollection(group.collectionId!) : undefined}
      className={cn(
        "flex h-full w-full items-center gap-3 rounded-md px-2 pt-3 text-left transition-colors",
        interactive ? "hover:bg-hover cursor-pointer" : "cursor-default",
      )}
    >
      <span className={cn(group.ghost && "opacity-50 grayscale")}>
        <Poster tmdbId={group.tmdbId} kind={group.kind ?? null} size="xs" alt={group.title} />
      </span>
      <span
        className={cn(
          "min-w-0 flex-1 truncate text-sm font-semibold",
          group.ghost && "text-muted-foreground",
        )}
        title={group.title}
      >
        {group.title}
      </span>
      {group.ghost && (
        <Badge variant="outline" className="h-4 shrink-0 px-1.5 text-[10px] text-muted-foreground">
          Gone from disk
        </Badge>
      )}
      {interactive && <ChevronRight className="size-4 shrink-0 text-muted-foreground" />}
    </button>
  );
}

/** Per-episode line under a header: "S01E03 · 43% · 2d ago". */
function EpisodeRow({
  item,
  onPlay,
  onRestore,
  restoringKey,
}: {
  item: Item;
  onPlay: (item: Item) => void;
  onRestore?: (item: Item) => void;
  restoringKey?: string | null;
}) {
  const playable = !item.deleted;
  const label = episodeLabel(item) ?? item.torrent_name;
  return (
    <div className="flex h-full items-center gap-3 pl-4 pr-2">
      <button
        type="button"
        disabled={!playable}
        onClick={playable ? () => onPlay(item) : undefined}
        className={cn(
          "flex h-full min-w-0 flex-1 items-center gap-3 rounded-md px-2 text-left transition-colors",
          playable ? "hover:bg-hover cursor-pointer" : "cursor-default opacity-60",
        )}
      >
        <span className="w-24 shrink-0 truncate text-sm font-medium tabular-nums" title={label}>
          {label}
        </span>
        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
          {statusLine(item)}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatRecentTime(item.last_watched_at)}
        </span>
      </button>
      <RestoreSlot item={item} onRestore={onRestore} restoringKey={restoringKey} />
    </div>
  );
}

/** Standalone (movie) line — header and row merged into one. */
function SoloRow({
  item,
  group,
  onPlay,
  onOpenCollection,
  onRestore,
  restoringKey,
}: {
  item: Item;
  group: Group;
  onPlay: (item: Item) => void;
  onOpenCollection?: (collectionId: string) => void;
  onRestore?: (item: Item) => void;
  restoringKey?: string | null;
}) {
  const playable = !item.deleted;
  const openable = group.collectionId != null && onOpenCollection != null;
  const interactive = playable || openable;
  return (
    <div className="flex h-full items-center gap-3 pr-2">
      <button
        type="button"
        disabled={!interactive}
        onClick={
          playable
            ? () => onPlay(item)
            : openable
              ? () => onOpenCollection(group.collectionId!)
              : undefined
        }
        className={cn(
          "flex h-full min-w-0 flex-1 items-center gap-3 rounded-md px-2 text-left transition-colors",
          interactive ? "hover:bg-hover cursor-pointer" : "cursor-default",
          item.deleted && "opacity-60",
        )}
      >
        <span className={cn(item.deleted && "opacity-70 grayscale")}>
          <Poster tmdbId={group.tmdbId} kind={group.kind ?? null} size="xs" alt={group.title} />
        </span>
        <div className="grid min-w-0 flex-1 gap-0.5">
          <span className="truncate text-sm font-medium" title={group.title}>
            {group.title}
          </span>
          <span className="flex items-center gap-2 text-xs text-muted-foreground">
            {statusLine(item)}
            {item.deleted && (
              <Badge variant="outline" className="h-4 px-1.5 text-[10px] text-muted-foreground">
                Gone from disk
              </Badge>
            )}
          </span>
        </div>
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatRecentTime(item.last_watched_at)}
        </span>
      </button>
      <RestoreSlot item={item} onRestore={onRestore} restoringKey={restoringKey} />
    </div>
  );
}

/** "Download again" on GC'd rows whose source release is re-resolvable.
 *  Same release → same infohash → the saved position resumes untouched. */
function RestoreSlot({
  item,
  onRestore,
  restoringKey,
}: {
  item: Item;
  onRestore?: (item: Item) => void;
  restoringKey?: string | null;
}) {
  if (!onRestore || !canRestore(item)) return null;
  const busy = restoringKey === itemKey(item);
  return (
    <button
      type="button"
      disabled={busy}
      onClick={() => onRestore(item)}
      className={cn(
        "flex shrink-0 items-center gap-1.5 rounded-md border border-border px-2 py-1 text-xs",
        "text-muted-foreground transition-colors hover:bg-hover hover:text-foreground",
        busy && "cursor-default opacity-50",
      )}
      title="Re-download this exact release and resume where you left off"
    >
      <RotateCcw className={cn("size-3", busy && "animate-spin")} />
      {busy ? "Restoring…" : "Download again"}
    </button>
  );
}
