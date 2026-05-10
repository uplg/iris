import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router";
import {
  BookmarkX,
  CheckCircle2,
  Download,
  Loader2,
  Play,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  follows,
  tmdbImage,
  type EpisodeItem,
  type EpisodesResponse,
  type FollowSummary,
} from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * SCENE-mode Series detail page.
 *
 * Routed by follow id (not tmdb_id). The episode list is built
 * entirely from server-side SCENE sources:
 *   - episode_files (on disk, joined via collections.parsed_title_normalized)
 *   - available_episodes (indexer cache, keyed on normalized_name)
 *
 * No TMDB call, no "expected" episode grid — episodes the indexer
 * doesn't know about don't appear here. That's correct: Iris can't
 * grab what isn't listed anyway. Posters surface only if the
 * matching collection is `tmdb_verified` (server gates the poster
 * URL inside the follow summary).
 */
export function SeriesPage() {
  const { followId } = useParams<{ followId: string }>();
  const id = followId!;
  const qc = useQueryClient();
  const navigate = useNavigate();

  const followsQ = useQuery({
    queryKey: ["follows"],
    queryFn: follows.list,
    staleTime: 60_000,
  });
  const follow: FollowSummary | undefined = useMemo(
    () => followsQ.data?.find((f) => f.id === id),
    [followsQ.data, id],
  );

  // Poll briefly for fresh follow data so the just-added follow's
  // initial scan results come in without forcing a manual refresh.
  const queryStartRef = useRef<number>(0);
  useEffect(() => {
    queryStartRef.current = 0;
  }, [id]);

  const episodesQ = useQuery({
    queryKey: ["follow-episodes", id],
    queryFn: () => follows.episodes(id),
    enabled: !!id && !!follow,
    refetchInterval: (q) => {
      const data = q.state.data as EpisodesResponse | undefined;
      if (!data) return false;
      if (queryStartRef.current === 0) queryStartRef.current = Date.now();
      const elapsedMs = Date.now() - queryStartRef.current;
      // Active scan window: poll fast for the first 60 s after page
      // load. After that, settle — the periodic 4 h scheduler will
      // pick up later releases.
      if (elapsedMs < 60_000 && data.items.length === 0) return 3_000;
      return false;
    },
  });

  const unfollowMutation = useMutation({
    mutationFn: () => follows.remove(id),
    onSuccess: () => {
      // Update the cache synchronously so the home Watchlist
      // tile is gone before we navigate — invalidate alone is
      // async and would briefly render the stale follow.
      qc.setQueryData<FollowSummary[]>(["follows"], (old) =>
        (old ?? []).filter((f) => f.id !== id),
      );
      void qc.invalidateQueries({ queryKey: ["follows"] });
      navigate("/", { replace: true });
    },
  });

  // ALL hooks must run before any early return — React tracks them
  // by call order. Splitting episodes into seasons stays here.
  const seasons = useMemo(() => {
    const grouped = new Map<number, EpisodeItem[]>();
    for (const ep of episodesQ.data?.items ?? []) {
      const arr = grouped.get(ep.season) ?? [];
      arr.push(ep);
      grouped.set(ep.season, arr);
    }
    return Array.from(grouped.entries())
      .sort(([a], [b]) => a - b)
      .map(([season, items]) => ({
        season,
        items: items.sort((a, b) => a.episode - b.episode),
      }));
  }, [episodesQ.data]);

  if (!id) return <p>Invalid follow id.</p>;
  if (followsQ.isLoading) {
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-3 animate-spin" />
        Loading…
      </p>
    );
  }
  if (!follow) {
    return (
      <p className="text-sm text-muted-foreground">
        This series is no longer in your watchlist.
      </p>
    );
  }

  return (
    <div className="grid gap-6">
      <Hero
        follow={follow}
        onUnfollow={() => unfollowMutation.mutate()}
        unfollowBusy={unfollowMutation.isPending}
      />

      <EpisodesList
        followId={id}
        seasons={seasons}
        loading={episodesQ.isLoading}
        error={episodesQ.error}
        empty={
          (episodesQ.data?.items.length ?? 0) === 0 && !episodesQ.isLoading
        }
        onPlay={(infohash, fileIdx) => navigate(`/watch/${infohash}/${fileIdx}`)}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------

function Hero({
  follow,
  onUnfollow,
  unfollowBusy,
}: {
  follow: FollowSummary;
  onUnfollow: () => void;
  unfollowBusy: boolean;
}) {
  // Server only fills poster_path / backdrop_path when the joined
  // collection has tmdb_verified=true. No verified collection →
  // text-only hero.
  const backdrop = tmdbImage(follow.backdrop_path, "original");
  const poster = tmdbImage(follow.poster_path, "w342");
  return (
    <section className="relative overflow-hidden rounded-xl border border-border bg-card/30">
      {backdrop && (
        <>
          <img
            src={backdrop}
            alt=""
            className="absolute inset-0 h-full w-full object-cover opacity-30"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-background via-background/60 to-transparent" />
        </>
      )}
      <div className="relative flex flex-wrap gap-6 p-6">
        {poster ? (
          <img
            src={poster}
            alt={follow.name}
            className="h-56 w-40 shrink-0 rounded-md border border-border object-cover shadow-lg"
          />
        ) : (
          <div className="flex h-56 w-40 shrink-0 items-center justify-center rounded-md border border-dashed border-border bg-card text-center text-xs text-muted-foreground">
            No verified poster
          </div>
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <h1 className="text-3xl font-semibold tracking-tight">
            {follow.name}
          </h1>
          <p className="text-xs text-muted-foreground">
            SCENE identity:{" "}
            <code className="rounded bg-card px-1.5 py-0.5 font-mono">
              {follow.normalized_name}
            </code>
          </p>
          <div className="mt-auto flex items-center gap-2">
            <Button
              variant="secondary"
              onClick={onUnfollow}
              disabled={unfollowBusy}
            >
              <BookmarkX className="size-4" />
              Unfollow
            </Button>
            <span className="text-xs text-muted-foreground">
              {follow.new_count > 0
                ? `${follow.new_count} new episode${follow.new_count > 1 ? "s" : ""} since your last visit`
                : "Up to date"}
            </span>
          </div>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Episode list
// ---------------------------------------------------------------------------

function EpisodesList({
  followId,
  seasons,
  loading,
  error,
  empty,
  onPlay,
}: {
  followId: string;
  seasons: { season: number; items: EpisodeItem[] }[];
  loading: boolean;
  error: unknown;
  empty: boolean;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const [activeSeason, setActiveSeason] = useState<number | null>(null);
  useEffect(() => {
    if (activeSeason == null && seasons.length > 0) {
      setActiveSeason(seasons[0].season);
    }
  }, [activeSeason, seasons]);

  if (loading) {
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-3 animate-spin" />
        Loading episodes…
      </p>
    );
  }
  if (error) {
    return (
      <p className="text-sm text-destructive">
        {error instanceof Error ? error.message : "Failed to load"}
      </p>
    );
  }
  if (empty) {
    return (
      <p className="text-sm text-muted-foreground">
        No episodes found yet. The scheduler runs every 4 h.
      </p>
    );
  }
  if (seasons.length === 0) return null;

  const current = seasons.find((s) => s.season === activeSeason) ?? seasons[0];

  return (
    <div className="grid gap-4">
      <SeasonTabs
        seasons={seasons.map((s) => s.season)}
        value={current.season}
        onChange={setActiveSeason}
      />
      <ul className="divide-y divide-border rounded-lg border border-border bg-card/30">
        {current.items.map((ep) => (
          <EpisodeRow
            key={`${ep.season}-${ep.episode}`}
            followId={followId}
            ep={ep}
            onPlay={onPlay}
          />
        ))}
      </ul>
    </div>
  );
}

function SeasonTabs({
  seasons,
  value,
  onChange,
}: {
  seasons: number[];
  value: number;
  onChange: (s: number) => void;
}) {
  if (seasons.length <= 1) return null;
  return (
    <div className="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1">
      {seasons.map((s) => (
        <button
          key={s}
          type="button"
          onClick={() => onChange(s)}
          className={cn(
            "rounded-md border px-3 py-1.5 text-sm transition",
            s === value
              ? "border-primary bg-primary/10 text-primary"
              : "border-border text-muted-foreground hover:border-border/80 hover:text-foreground",
          )}
        >
          Season {s}
        </button>
      ))}
    </div>
  );
}

function EpisodeRow({
  followId,
  ep,
  onPlay,
}: {
  followId: string;
  ep: EpisodeItem;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const qc = useQueryClient();
  const grabAndPlay = useMutation({
    mutationFn: () => follows.grabEpisode(followId, ep.season, ep.episode),
    onSuccess: (res) => {
      void qc.invalidateQueries({ queryKey: ["follow-episodes", followId] });
      onPlay(res.infohash, res.file_idx);
    },
  });
  const grabOnly = useMutation({
    mutationFn: () => follows.grabEpisode(followId, ep.season, ep.episode),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["follow-episodes", followId] });
    },
  });

  return (
    <li className="grid grid-cols-[3rem_1fr_auto] items-center gap-3 px-4 py-3 text-sm">
      <span className="text-center font-mono text-muted-foreground">
        {ep.episode.toString().padStart(2, "0")}
      </span>
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">
            S{ep.season.toString().padStart(2, "0")}E
            {ep.episode.toString().padStart(2, "0")}
          </span>
          <StatusBadge ep={ep} />
        </div>
        <div className="mt-0.5 flex items-center gap-3 text-xs text-muted-foreground">
          {ep.quality && <span>{ep.quality}</span>}
          {ep.seeders != null && <span>{ep.seeders} seeders</span>}
        </div>
      </div>
      <EpisodeAction
        ep={ep}
        onPlay={onPlay}
        onGrabAndPlay={() => grabAndPlay.mutate()}
        onGrabOnly={() => grabOnly.mutate()}
        grabBusy={grabAndPlay.isPending || grabOnly.isPending}
      />
    </li>
  );
}

function StatusBadge({ ep }: { ep: EpisodeItem }) {
  if (ep.watched) {
    return (
      <Badge variant="secondary" className="text-[10px]">
        <CheckCircle2 className="mr-1 size-3" /> watched
      </Badge>
    );
  }
  if (ep.status === "downloaded") {
    return (
      <Badge variant="secondary" className="text-[10px]">
        downloaded
      </Badge>
    );
  }
  return <Badge className="bg-emerald-500/80 text-[10px]">available</Badge>;
}

function EpisodeAction({
  ep,
  onPlay,
  onGrabAndPlay,
  onGrabOnly,
  grabBusy,
}: {
  ep: EpisodeItem;
  onPlay: (infohash: string, fileIdx: number) => void;
  onGrabAndPlay: () => void;
  onGrabOnly: () => void;
  grabBusy: boolean;
}) {
  if (ep.status === "downloaded" && ep.infohash != null && ep.file_idx != null) {
    return (
      <Button size="sm" onClick={() => onPlay(ep.infohash!, ep.file_idx!)}>
        <Play className="size-3.5" />
        {ep.watched ? "Watch again" : "Play"}
      </Button>
    );
  }
  return (
    <div className="flex items-center gap-1">
      <Button size="sm" variant="secondary" onClick={onGrabOnly} disabled={grabBusy}>
        <Download className="size-3.5" />
        Prepare
      </Button>
      <Button size="sm" onClick={onGrabAndPlay} disabled={grabBusy}>
        {grabBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
        Play
      </Button>
    </div>
  );
}
