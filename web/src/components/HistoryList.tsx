import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Badge } from "@/components/ui/badge";
import { Poster } from "@/components/Poster";
import type { HistoryItem, UserHistoryItem } from "@/lib/api";
import { formatRecentTime, formatTimecode } from "@/lib/format";
import { cn } from "@/lib/utils";

type Item = HistoryItem | UserHistoryItem;

const ROW_HEIGHT = 56;

/** What the user actually watched: the file's basename (the episode, for a
 *  season pack) when we have it, else the torrent name. */
function watchedLabel(item: Item): string {
  const base = item.file_path ? (item.file_path.split("/").pop() ?? item.file_path) : null;
  return base ?? item.torrent_name;
}

function statusLine(item: Item): string {
  if (item.deleted) return "No longer available";
  if (item.completed) return "Watched";
  const pct =
    item.duration_seconds && item.duration_seconds > 0
      ? Math.min(100, (item.position_seconds / item.duration_seconds) * 100)
      : 0;
  return pct > 0 ? `${pct.toFixed(0)}% · ${formatTimecode(item.position_seconds)}` : "Just started";
}

/**
 * One row per episode, newest first — a scannable log, not a poster wall.
 * Shared by the user-facing Watch History page (`me.history()`) and the
 * admin per-user drill-down (`admin.userHistory()`); both endpoints return
 * the same row shape, just scoped differently server-side.
 *
 * Virtualized (same pattern as `AdminPage`'s `UserList` / `AuditLog`) so the
 * list stays smooth no matter how many episodes have piled up — rows are a
 * fixed height, so a constant `estimateSize` is exact.
 *
 * Deleted-source rows (the torrent was GC'd / admin-removed) render
 * non-interactive with a "Removed" badge instead of a resume link — the
 * whole point of this list is that they stay visible, not playable.
 */
export function HistoryList({ items, onPlay }: { items: Item[]; onPlay: (item: Item) => void }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
    getItemKey: (i) => `${items[i]!.infohash}:${items[i]!.file_idx}`,
  });

  return (
    <div ref={parentRef} className="max-h-[70svh] overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((v) => {
          const it = items[v.index]!;
          const interactive = !it.deleted;
          return (
            <div
              key={v.key}
              data-index={v.index}
              className="absolute inset-x-0"
              style={{ top: 0, height: v.size, transform: `translateY(${v.start}px)` }}
            >
              <button
                type="button"
                disabled={!interactive}
                onClick={interactive ? () => onPlay(it) : undefined}
                className={cn(
                  "flex h-full w-full items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors",
                  interactive ? "hover:bg-hover cursor-pointer" : "cursor-default opacity-60",
                )}
              >
                <Poster
                  tmdbId={it.tmdb_id}
                  kind={it.kind ?? null}
                  size="xs"
                  alt={it.torrent_name}
                />
                <div className="grid min-w-0 flex-1 gap-0.5">
                  <span className="truncate text-sm font-medium" title={watchedLabel(it)}>
                    {watchedLabel(it)}
                  </span>
                  <span className="flex items-center gap-2 text-xs text-muted-foreground">
                    {statusLine(it)}
                    {it.deleted && (
                      <Badge variant="destructive" className="h-4 px-1.5 text-[10px]">
                        Removed
                      </Badge>
                    )}
                  </span>
                </div>
                <span className="shrink-0 text-xs text-muted-foreground">
                  {formatRecentTime(it.last_watched_at)}
                </span>
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
