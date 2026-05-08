import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router";
import { CheckCircle2, ChevronDown, ChevronUp, Download, Play, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { me, torrents, type ContinueWatchingItem, type TorrentView } from "@/lib/api";
import { formatSize } from "@/lib/format";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

export function LibraryPage() {
  const qc = useQueryClient();
  const { data, isLoading, error } = useQuery({
    queryKey: ["torrents"],
    queryFn: torrents.list,
    refetchInterval: 3000,
  });
  const cwQ = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });

  const remove = useMutation({
    mutationFn: (infohash: string) => torrents.remove(infohash),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["torrents"] });
      void qc.invalidateQueries({ queryKey: ["continue-watching"] });
    },
  });

  return (
    <div className="grid gap-6">
      <section>
        <h1 className="text-3xl font-semibold tracking-tight">Library</h1>
        <p className="mt-1 text-muted-foreground">Active downloads and seeded torrents.</p>
      </section>

      {isLoading && <p className="text-sm text-muted-foreground">Loading…</p>}
      {error && (
        <p className="text-sm text-destructive">
          {error instanceof Error ? error.message : "failed to load"}
        </p>
      )}
      {data && data.length === 0 && (
        <p className="text-sm text-muted-foreground">
          Nothing yet. Search for something on the home page.
        </p>
      )}
      <div className="grid gap-3">
        {data?.map((t) => (
          <LibraryRow
            key={t.infohash}
            t={t}
            progress={cwQ.data ?? []}
            onRemove={() => remove.mutate(t.infohash)}
            removing={remove.isPending}
          />
        ))}
      </div>
    </div>
  );
}

function LibraryRow({
  t,
  progress,
  onRemove,
  removing,
}: {
  t: TorrentView;
  progress: ContinueWatchingItem[];
  onRemove: () => void;
  removing: boolean;
}) {
  const pct = Math.min(100, Math.max(0, t.progress_pct));
  const videos = t.files.filter((f) => VIDEO_RE.test(f.path));
  const [expanded, setExpanded] = useState(false);
  const progressByFileIdx = new Map<number, ContinueWatchingItem>(
    progress.filter((p) => p.infohash === t.infohash).map((p) => [p.file_idx, p]),
  );

  const titleClass = "min-w-0 break-words font-medium";

  return (
    <div className="rounded-lg border border-border bg-card/40 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className={titleClass} title={t.name ?? undefined}>
              {t.name ?? t.infohash}
            </h3>
            <StateBadge state={t.state} />
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {formatSize(t.progress_bytes)} / {formatSize(t.total_size_bytes)} · ↓{" "}
            {formatSize(t.download_speed_bps)}/s · ↑ {formatSize(t.upload_speed_bps)}/s · {t.peers}{" "}
            peers
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {videos.length === 1 && (
            <Button asChild size="sm" variant="outline">
              <Link to={`/watch/${t.infohash}/${videos[0]!.index}`}>
                <Play className="size-3.5" />
                Play
              </Link>
            </Button>
          )}
          {videos.length > 1 && (
            <Button size="sm" variant="outline" onClick={() => setExpanded((v) => !v)}>
              {expanded ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
              {expanded ? "Hide files" : `${videos.length} files`}
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={onRemove}
            disabled={removing}
            title="Remove torrent"
          >
            <Trash2 className="size-3.5" />
            <span className="sr-only">Remove</span>
          </Button>
        </div>
      </div>
      <Progress className="mt-3" value={pct} />
      {t.error && <p className="mt-2 text-xs text-destructive">{t.error}</p>}
      {expanded && videos.length > 1 && (
        <ul className="mt-3 grid gap-1 border-t border-border pt-3 text-sm">
          {videos.map((f) => {
            const fname = f.path.split("/").pop() ?? f.path;
            const watch = progressByFileIdx.get(f.index);
            const watchedPct =
              watch && watch.duration_seconds && watch.duration_seconds > 0
                ? Math.min(100, (watch.position_seconds / watch.duration_seconds) * 100)
                : null;
            return (
              <li
                key={f.index}
                className="flex items-center justify-between gap-3 rounded px-2 py-1.5 hover:bg-muted/40"
              >
                <div className="min-w-0 flex-1">
                  <div className="break-all font-mono text-xs">{f.path}</div>
                  <div className="mt-0.5 flex items-center gap-3 text-[11px] text-muted-foreground">
                    <span>{formatSize(f.size_bytes)}</span>
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
                    <Link to={`/watch/${t.infohash}/${f.index}`}>
                      <Play className="size-3.5" />
                      {watchedPct != null && watchedPct > 0 && !watch?.completed
                        ? "Resume"
                        : "Play"}
                    </Link>
                  </Button>
                  <Button asChild size="sm" variant="outline" title="Download">
                    <a href={torrents.downloadUrl(t.infohash, f.index)} download={fname}>
                      <Download className="size-3.5" />
                      <span className="sr-only">Download</span>
                    </a>
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
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
