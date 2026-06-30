import { useQuery } from "@tanstack/react-query";
import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ScrollText } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { admin, type AuditLogEntry } from "@/lib/api";
import { formatRecentTime } from "@/lib/format";

/** Max rows shown. The list is virtualized, so this bounds the request, not
 *  the render cost. */
const LOG_LIMIT = 50;

/** "torrent.delete" → "deleted a torrent" — readable past-tense phrasing
 *  per action. Falls back to the raw action string for anything not
 *  listed here, so a newly-added audited action never renders as blank. */
const ACTION_LABELS: Record<string, string> = {
  "torrent.delete": "deleted a torrent",
  "user.password_reset": "reset a password",
  "user.display_name_update": "changed a display name",
  "gc.evict": "ran garbage collection",
  "remux.wipe": "wiped a remux cache entry",
};

function actionLabel(action: string): string {
  return ACTION_LABELS[action] ?? action;
}

/**
 * Virtualized so the log stays smooth even once it's accumulated months of
 * entries — same pattern as `AdminPage`'s `UserList`. Rows are a fixed
 * height (the details line always renders, empty or not), so a constant
 * `estimateSize` is exact and no `measureElement` is needed.
 */
function AuditLogList({ entries }: { entries: AuditLogEntry[] }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const ROW_HEIGHT = 52;
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    getItemKey: (i) => entries[i]!.id,
  });

  return (
    <div ref={parentRef} className="max-h-[26rem] overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((v) => {
          const entry = entries[v.index]!;
          return (
            <div
              key={v.key}
              data-index={v.index}
              className="absolute inset-x-0 flex items-start gap-3 rounded-md px-2 py-1.5"
              style={{ top: 0, height: v.size, transform: `translateY(${v.start}px)` }}
            >
              <div className="grid min-w-0 flex-1 gap-0.5">
                <span className="truncate text-sm">
                  <span className="font-medium">{entry.actor_display_name}</span>{" "}
                  <span className="text-muted-foreground">{actionLabel(entry.action)}</span>
                </span>
                <span
                  className="truncate text-xs text-muted-foreground"
                  title={entry.details ?? ""}
                >
                  {entry.details ?? " "}
                </span>
              </div>
              <span className="shrink-0 text-xs text-muted-foreground">
                {formatRecentTime(entry.created_at)}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Persisted "who changed/deleted what" log — deletions, password resets,
 * admin-triggered GC. Replaces the previous ephemeral `tracing::` logs,
 * which rotated out and weren't queryable from here.
 */
export function AuditLog() {
  const log = useQuery({
    queryKey: ["admin", "audit-log"],
    queryFn: () => admin.auditLog(LOG_LIMIT),
    refetchInterval: 30_000,
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ScrollText className="size-4" />
          Audit log
          {log.data && log.data.length > 0 ? (
            <Badge variant="secondary" className="ml-1">
              {log.data.length}
            </Badge>
          ) : null}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {log.data && log.data.length > 0 ? (
          <AuditLogList entries={log.data} />
        ) : (
          <p className="text-sm text-muted-foreground">
            {log.isLoading ? "Loading…" : "No audited actions yet."}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
