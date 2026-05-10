import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Download,
  LayoutGrid,
  List,
  Play,
  Trash2,
} from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { EmptyState, ErrorState, SkeletonCard } from "@/components/State";
import {
  library,
  me,
  torrents,
  type CollectionListItem,
  type ContinueWatchingItem,
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
          // No TMDB lookup on library cards — even tmdb_verified
          // collections have shown the wrong poster in the past
          // (runtime probe within ±15% can false-match an unrelated
          // title). SCENE display title is the truth; the kind
          // placeholder (Film / Tv icon) carries the rest.
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
// Torrents view (legacy)
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

  if (isLoading) return <SkeletonCard count={3} />;
  if (error) return <ErrorState error={error} />;
  const items: TorrentView[] = data && data.view === "torrents" ? data.items : [];
  if (items.length === 0)
    return (
      <EmptyState
        title="No torrents"
        body="Library is empty for now."
      />
    );
  return (
    <div className="grid gap-3">
      {items.map((t) => (
        <TorrentRow
          key={t.infohash}
          t={t}
          progress={cwQ.data ?? []}
          onRemove={() => remove.mutate(t.infohash)}
          removing={remove.isPending}
        />
      ))}
    </div>
  );
}

function TorrentRow({
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
          <p className="mt-0.5 text-xs text-muted-foreground">
            Added by <span className="text-foreground">{t.added_by_name}</span>
            {" · "}
            {new Date(t.added_at).toLocaleDateString()}
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
