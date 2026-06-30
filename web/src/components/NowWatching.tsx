import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Eye, Monitor, Pause, Play, Tv } from "lucide-react";

import { Poster } from "@/components/Poster";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { admin, type ActiveSession, type WatchHistoryEntry } from "@/lib/api";
import { formatRecentTime } from "@/lib/format";

/** Max rows shown in "Recent activity". The list is scrollable, so this
 *  bounds the request, not the page height. */
const HISTORY_LIMIT = 30;

function pct(position: number, duration: number | null): number {
  return duration && duration > 0 ? Math.min(100, (position / duration) * 100) : 0;
}

/** What the user is actually watching: the file's basename (the episode,
 *  for a season pack) when we have it, else the torrent name. */
function watchedLabel(filePath: string | null, torrentName: string | null): string {
  const base = filePath ? (filePath.split("/").pop() ?? filePath) : null;
  return base ?? torrentName ?? "Unknown";
}

/** Elapsed watch time since the session started, as "for 12m". */
function elapsed(iso: string): string {
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return "for <1m";
  if (secs < 3600) return `for ${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return m > 0 ? `for ${h}h ${m}m` : `for ${h}h`;
}

function ClientIcon({ client }: { client: ActiveSession["client"] }) {
  if (client === "tv") return <Tv className="size-3.5" />;
  if (client === "web") return <Monitor className="size-3.5" />;
  return null;
}

function SessionCard({ s }: { s: ActiveSession }) {
  const progress = pct(s.position_seconds, s.duration_seconds ?? null);
  return (
    <Link
      to="/watch/$infohash/$idx"
      params={{ infohash: s.infohash, idx: String(s.file_idx) }}
      className="flex gap-3 rounded-lg border border-border bg-elev/50 p-3 transition-colors hover:bg-elev"
    >
      <Poster tmdbId={s.tmdb_id} kind={s.kind ?? null} size="md" alt={s.torrent_name ?? ""} />
      <div className="grid min-w-0 flex-1 content-between gap-2">
        <div className="grid gap-1">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{s.display_name}</span>
            {s.state === "playing" ? (
              <Badge variant="secondary" className="gap-1 text-emerald-500">
                <Play className="size-3 fill-current" />
                Playing
              </Badge>
            ) : (
              <Badge variant="outline" className="gap-1 text-fg-dim">
                <Pause className="size-3 fill-current" />
                Paused
              </Badge>
            )}
            <span className="ml-auto flex items-center gap-1.5 text-xs text-muted-foreground">
              <span className="flex items-center gap-1" title={s.client ?? undefined}>
                <ClientIcon client={s.client} />
                {s.client_version ?? ""}
              </span>
              <span>·</span>
              <span>{elapsed(s.started_at)}</span>
            </span>
          </div>
          <span
            className="truncate text-sm text-muted-foreground"
            title={watchedLabel(s.file_path ?? null, s.torrent_name ?? null)}
          >
            {watchedLabel(s.file_path ?? null, s.torrent_name ?? null)}
          </span>
        </div>
        <div className="grid gap-1">
          <Progress value={progress} />
          <span className="text-xs text-muted-foreground">{progress.toFixed(0)}% watched</span>
        </div>
      </div>
    </Link>
  );
}

function HistoryRow({ h }: { h: WatchHistoryEntry }) {
  const progress = pct(h.position_seconds, h.duration_seconds ?? null);
  return (
    <Link
      to="/watch/$infohash/$idx"
      params={{ infohash: h.infohash, idx: String(h.file_idx) }}
      className="flex items-center gap-3 rounded-md px-2 py-1.5 transition-colors hover:bg-elev"
    >
      <Poster tmdbId={h.tmdb_id} kind={h.kind ?? null} size="xs" alt={h.torrent_name} />
      <div className="grid min-w-0 flex-1">
        <span className="truncate text-sm">
          <span className="font-medium">{h.display_name}</span>
          <span className="text-muted-foreground">
            {" · "}
            {watchedLabel(h.file_path ?? null, h.torrent_name)}
          </span>
        </span>
        <span className="text-xs text-muted-foreground">
          {h.completed ? "Finished" : `${progress.toFixed(0)}%`} ·{" "}
          {formatRecentTime(h.last_watched_at)}
        </span>
      </div>
      {h.completed ? (
        <Badge variant="secondary" className="shrink-0">
          Done
        </Badge>
      ) : null}
    </Link>
  );
}

/**
 * Admin "Now watching" + "Recent activity" panels. Live sessions come from
 * the in-memory presence registry (fed by the existing progress heartbeat),
 * polled every 5s via React Query's `refetchInterval` — no hand-rolled timer.
 */
export function NowWatching() {
  const sessions = useQuery({
    queryKey: ["admin", "active-sessions"],
    queryFn: admin.activeSessions,
    refetchInterval: 5_000,
  });
  const history = useQuery({
    queryKey: ["admin", "watch-history"],
    queryFn: () => admin.watchHistory(HISTORY_LIMIT),
    refetchInterval: 30_000,
  });

  const live = sessions.data ?? [];

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Eye className="size-4" />
            Now watching
            {live.length > 0 ? (
              <Badge variant="secondary" className="ml-1">
                {live.length}
              </Badge>
            ) : null}
          </CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3">
          {live.length > 0 ? (
            live.map((s) => <SessionCard key={s.user_id} s={s} />)
          ) : (
            <p className="text-sm text-muted-foreground">
              {sessions.isLoading ? "Loading…" : "Nobody is watching right now."}
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Recent activity</CardTitle>
        </CardHeader>
        <CardContent className="grid max-h-[26rem] gap-0.5 overflow-y-auto">
          {history.data && history.data.length > 0 ? (
            history.data.map((h) => (
              <HistoryRow key={`${h.user_id}:${h.infohash}:${h.file_idx}`} h={h} />
            ))
          ) : (
            <p className="text-sm text-muted-foreground">
              {history.isLoading ? "Loading…" : "No playback recorded yet."}
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
