import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router";
import {
  Bookmark,
  BookmarkCheck,
  CheckCircle2,
  Clock,
  Download,
  Loader2,
  Play,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  follows,
  metadata,
  tmdbImage,
  type EpisodeItem,
  type EpisodesResponse,
  type FollowSummary,
} from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * Series detail page. Shows season tabs, episode list with status, and
 * a follow toggle. Each episode has the appropriate primary action
 * inline (Lire / Préparer / Revoir / À venir) so the user never has to
 * leave the page to start playing — except when status is `available`,
 * where the click triggers an ingest then navigates to the player.
 *
 * Auto-grab is intentionally NOT a feature; user always confirms by
 * clicking. The notify scheduler pre-caches indexer hits in the
 * background so these clicks resolve instantly.
 */
export function SeriesPage() {
  const { tmdbId } = useParams<{ tmdbId: string }>();
  const id = Number(tmdbId);
  const qc = useQueryClient();
  const navigate = useNavigate();

  const tmdbQ = useQuery({
    queryKey: ["tmdb", id],
    queryFn: () => metadata.tmdb(id),
    enabled: Number.isFinite(id),
    staleTime: Infinity,
  });

  // Watchlist lookup — drives the Follow / Suivi state on the hero.
  // Cached for 60 s; mutations invalidate explicitly.
  const followsQ = useQuery({
    queryKey: ["follows"],
    queryFn: follows.list,
    staleTime: 60_000,
  });
  const followed: FollowSummary | undefined = useMemo(
    () => followsQ.data?.find((f) => f.tmdb_id === id),
    [followsQ.data, id],
  );

  const totalSeasons =
    followed?.total_seasons ?? tmdbQ.data?.number_of_seasons ?? 1;
  const [season, setSeason] = useState(1);
  // Reset to season 1 when navigating to a different series.
  useEffect(() => {
    setSeason(1);
  }, [id]);

  // Track when this season's query first ran so we can poll briefly
  // after the user follows — the backend's notify scan is asynchronous
  // and takes a few seconds to populate `available_episodes`. Without
  // polling, the user sees "pas dispo" everywhere until they hit
  // refresh manually.
  const queryStartRef = useRef<number>(0);
  useEffect(() => {
    queryStartRef.current = 0;
  }, [id, season]);

  const episodesQ = useQuery({
    queryKey: ["follow-episodes", id, season],
    queryFn: () => follows.episodes(id, season),
    enabled: Number.isFinite(id) && !!followed, // only useful once the user is following
    refetchInterval: (q) => {
      const data = q.state.data as EpisodesResponse | undefined;
      if (!data) return false;
      if (queryStartRef.current === 0) queryStartRef.current = Date.now();
      const hasUnavailable = data.items.some((e) => e.status === "unavailable");
      const elapsedMs = Date.now() - queryStartRef.current;
      // Active scan window: poll fast for the first 60 s after the
      // page opens so the background scan's results show up without
      // the user having to refresh. After that, settle.
      if (hasUnavailable && elapsedMs < 60_000) return 3_000;
      return false;
    },
  });

  const followMutation = useMutation({
    mutationFn: () =>
      follows.add(
        id,
        tmdbQ.data?.title,
        tmdbQ.data?.number_of_seasons ?? undefined,
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["follows"] }),
  });
  const unfollowMutation = useMutation({
    mutationFn: () => follows.remove(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["follows"] }),
  });

  if (!Number.isFinite(id)) return <p>Identifiant TMDB invalide.</p>;

  return (
    <div className="grid gap-6">
      <Hero
        meta={tmdbQ.data}
        followed={!!followed}
        onFollow={() => followMutation.mutate()}
        onUnfollow={() => unfollowMutation.mutate()}
        followBusy={followMutation.isPending || unfollowMutation.isPending}
      />

      {!followed ? (
        <div className="rounded-md border border-dashed border-border bg-card/40 p-6 text-sm text-muted-foreground">
          Suis cette série pour voir les épisodes attendus, ce qui est dispo et ce qui
          manque.
        </div>
      ) : (
        <>
          <SeasonTabs total={totalSeasons} value={season} onChange={setSeason} />
          <EpisodesList
            tmdbId={id}
            season={season}
            data={episodesQ.data}
            loading={episodesQ.isLoading}
            error={episodesQ.error}
            onPlay={(infohash, fileIdx) => navigate(`/watch/${infohash}/${fileIdx}`)}
          />
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------

function Hero({
  meta,
  followed,
  onFollow,
  onUnfollow,
  followBusy,
}: {
  meta: { title?: string; overview?: string | null; poster_path?: string | null; backdrop_path?: string | null; year?: number | null } | undefined;
  followed: boolean;
  onFollow: () => void;
  onUnfollow: () => void;
  followBusy: boolean;
}) {
  const backdrop = tmdbImage(meta?.backdrop_path, "original");
  const poster = tmdbImage(meta?.poster_path, "w342");
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
        {poster && (
          <img
            src={poster}
            alt={meta?.title ?? ""}
            className="h-56 w-40 shrink-0 rounded-md border border-border object-cover shadow-lg"
          />
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <h1 className="text-3xl font-semibold tracking-tight">
            {meta?.title ?? "Chargement…"}
            {meta?.year && (
              <span className="ml-2 text-base font-normal text-muted-foreground">
                ({meta.year})
              </span>
            )}
          </h1>
          {meta?.overview && (
            <p className="max-w-3xl text-sm leading-relaxed text-muted-foreground">
              {meta.overview}
            </p>
          )}
          <div className="mt-auto flex items-center gap-2">
            <Button
              variant={followed ? "secondary" : "default"}
              onClick={followed ? onUnfollow : onFollow}
              disabled={followBusy}
            >
              {followed ? (
                <>
                  <BookmarkCheck className="size-4" />
                  Suivi
                </>
              ) : (
                <>
                  <Bookmark className="size-4" />
                  Suivre
                </>
              )}
            </Button>
            <span className="text-xs text-muted-foreground">
              {followed
                ? "Tu seras notifié quand de nouveaux épisodes sortent."
                : "Suis pour être notifié des nouveaux épisodes."}
            </span>
          </div>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Season tabs
// ---------------------------------------------------------------------------

function SeasonTabs({
  total,
  value,
  onChange,
}: {
  total: number;
  value: number;
  onChange: (s: number) => void;
}) {
  if (total <= 1) return null;
  return (
    <div className="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1">
      {Array.from({ length: total }, (_, i) => i + 1).map((s) => (
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
          Saison {s}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Episode list
// ---------------------------------------------------------------------------

function EpisodesList({
  tmdbId,
  season,
  data,
  loading,
  error,
  onPlay,
}: {
  tmdbId: number;
  season: number;
  data: EpisodesResponse | undefined;
  loading: boolean;
  error: unknown;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  if (loading) {
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-3 animate-spin" />
        Chargement des épisodes…
      </p>
    );
  }
  if (error) {
    return (
      <p className="text-sm text-destructive">
        {error instanceof Error ? error.message : "Échec du chargement"}
      </p>
    );
  }
  if (!data || data.items.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        TMDB n'a pas encore listé les épisodes de la saison {season}.
      </p>
    );
  }
  return (
    <ul className="divide-y divide-border rounded-lg border border-border bg-card/30">
      {data.items.map((ep) => (
        <EpisodeRow key={`${ep.season}-${ep.episode}`} tmdbId={tmdbId} ep={ep} onPlay={onPlay} />
      ))}
    </ul>
  );
}

function EpisodeRow({
  tmdbId,
  ep,
  onPlay,
}: {
  tmdbId: number;
  ep: EpisodeItem;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const qc = useQueryClient();
  const grabAndPlay = useMutation({
    mutationFn: () => follows.grabEpisode(tmdbId, ep.season, ep.episode),
    onSuccess: (res) => {
      // Refresh episodes so the row flips to "downloaded" on return
      // even before the polling tick.
      void qc.invalidateQueries({ queryKey: ["follow-episodes", tmdbId] });
      onPlay(res.infohash, res.file_idx);
    },
  });
  const grabOnly = useMutation({
    mutationFn: () => follows.grabEpisode(tmdbId, ep.season, ep.episode),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["follow-episodes", tmdbId] });
    },
  });

  return (
    <li className="grid grid-cols-[3rem_1fr_auto] items-center gap-3 px-4 py-3 text-sm">
      <span className="text-center font-mono text-muted-foreground">
        {ep.episode.toString().padStart(2, "0")}
      </span>
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">{ep.name ?? `Épisode ${ep.episode}`}</span>
          <StatusBadge ep={ep} />
        </div>
        <div className="mt-0.5 flex items-center gap-3 text-xs text-muted-foreground">
          {ep.air_date && <span>{ep.air_date}</span>}
          {ep.runtime_minutes && <span>{ep.runtime_minutes} min</span>}
          {ep.overview && <span className="truncate">· {ep.overview}</span>}
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
        <CheckCircle2 className="mr-1 size-3" /> vu
      </Badge>
    );
  }
  switch (ep.status) {
    case "downloaded":
      return (
        <Badge variant="secondary" className="text-[10px]">
          téléchargé
        </Badge>
      );
    case "available":
      return (
        <Badge className="bg-emerald-500/80 text-[10px]">
          dispo
        </Badge>
      );
    case "unavailable":
      return (
        <Badge variant="outline" className="text-[10px]">
          <Clock className="mr-1 size-3" />
          {ep.air_date && ep.air_date > new Date().toISOString().slice(0, 10)
            ? "à venir"
            : "pas dispo"}
        </Badge>
      );
  }
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
        {ep.watched ? "Revoir" : "Lire"}
      </Button>
    );
  }
  if (ep.status === "available") {
    return (
      <div className="flex items-center gap-1">
        <Button size="sm" variant="secondary" onClick={onGrabOnly} disabled={grabBusy}>
          <Download className="size-3.5" />
          Préparer
        </Button>
        <Button size="sm" onClick={onGrabAndPlay} disabled={grabBusy}>
          {grabBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
          Lire
        </Button>
      </div>
    );
  }
  return (
    <Button size="sm" variant="ghost" disabled>
      À venir
    </Button>
  );
}
