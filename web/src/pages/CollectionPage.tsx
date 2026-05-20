import { useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Download, Loader2, Play } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { LanguageBadge } from "@/components/LanguageBadge";
import {
  library,
  tmdbImage,
  type AvailableEpisodeEntry,
  type CollectionDetail,
  type CollectionEpisodeEntry,
} from "@/lib/api";
import { formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/**
 * Unified collection view — single surface for any series the
 * household has at least one episode of. Replaces the file-list-only
 * page and absorbs the responsibilities of the retired /series/:id
 * route. Per the workstream B plan: no Follow button. The collections
 * scheduler auto-arms the indexer watch as soon as the first
 * episode is ingested, so "follow" is implicit.
 *
 * Movies with a single playable file still auto-redirect to /watch
 * (no value in showing a one-file picker). TV shows render an
 * episode list merging:
 *   - episodes already on disk (Play action)
 *   - available_episodes the indexer has cached (Grab + Play /
 *     Prepare actions)
 *
 * Both lists arrive in a single CollectionDetail payload — no
 * second round-trip, the server filters out already-owned offers.
 */
export function CollectionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data, isLoading, error } = useQuery({
    queryKey: ["collection", id],
    queryFn: () => library.collection(id!),
    enabled: !!id,
  });

  // Auto-navigate movies straight to /watch when there's a single
  // playable file. Saves a useless intermediate click. TV stays on
  // this page even when only one episode is on disk — the new
  // "grab next episode" surface is the point of stopping here.
  useEffect(() => {
    if (!data) return;
    if (data.kind === "movie") {
      const t = data.torrents[0];
      const f = t?.files
        .filter((x) => VIDEO_RE.test(x.path))
        .sort((a, b) => b.size_bytes - a.size_bytes)[0];
      if (t && f) {
        navigate(`/watch/${t.infohash}/${f.index}`, { replace: true });
      }
    }
  }, [data, navigate]);

  if (isLoading) {
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-3 animate-spin" />
        Loading collection…
      </p>
    );
  }
  if (error) {
    return (
      <p className="text-sm text-destructive">
        {error instanceof Error ? error.message : "failed"}
      </p>
    );
  }
  if (!data) return null;

  // SCENE parser misses (Plex-style `NxNN` file names, exotic
  // numbering schemes, etc.) leave a TV collection with zero
  // `episode_files` rows. Don't show "no episodes" and orphan the
  // user — fall back to the raw file picker so they can still
  // play whatever is on disk. The parser improvement is tracked
  // separately; this is the cheap UX guard until then.
  const tvHasEpisodes =
    data.kind === "tv" &&
    (data.episodes.length > 0 || (data.available_episodes?.length ?? 0) > 0);

  return (
    <div className="grid gap-6">
      <Hero collection={data} />
      {tvHasEpisodes ? (
        <EpisodeList collection={data} onPlay={(ih, idx) => navigate(`/watch/${ih}/${idx}`)} />
      ) : (
        // Movies, or a TV pack the SCENE parser couldn't break into
        // episodes. Either way the raw file picker gets the user
        // unblocked.
        <RawFileList collection={data} onPlay={(ih, idx) => navigate(`/watch/${ih}/${idx}`)} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------

function Hero({ collection }: { collection: CollectionDetail }) {
  // Server-resolved poster + backdrop. Both `null` when the
  // collection has no tmdb_id or the TMDB lookup failed — the hero
  // falls back to a placeholder square without rendering broken
  // `<img>`. Backdrop fades behind the poster for visual depth.
  const poster = tmdbImage(collection.poster_path, "w342");
  const backdrop = tmdbImage(collection.backdrop_path, "original");
  const newCount = collection.has_new_since_last_visit ?? 0;
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
            alt={collection.display_title}
            className="h-56 w-40 shrink-0 rounded-md border border-border object-cover shadow-lg"
          />
        ) : (
          <div className="flex h-56 w-40 shrink-0 items-center justify-center rounded-md border border-dashed border-border bg-card text-center text-xs text-muted-foreground">
            No poster
          </div>
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <h1 className="text-3xl font-semibold tracking-tight">
            {collection.display_title}
          </h1>
          <p className="text-xs text-muted-foreground">
            {collection.kind === "tv" ? "Series" : "Movie"} ·{" "}
            {collection.torrents.length} torrent
            {collection.torrents.length > 1 ? "s" : ""}
            {newCount > 0 && (
              <>
                {" · "}
                <Badge className="bg-emerald-500/80 text-[10px] uppercase">
                  {newCount} new
                </Badge>
              </>
            )}
          </p>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Episode list — merged on-disk + indexer offers
// ---------------------------------------------------------------------------

/** A single episode row aggregates every variant we have for that
 *  (S, E) — owned releases + grabbable indexer offers — so a user
 *  with the FR release on disk and an EN release available in cache
 *  sees one row with TWO chips, not two separate rows. The chip the
 *  user clicks decides whether we Play (downloaded) or Grab
 *  (available, with the right language). */
type MergedEpisode = {
  season: number;
  episode: number;
  variants: EpisodeVariant[];
};

type EpisodeVariant =
  | {
      status: "downloaded";
      language: string | null;
      infohash: string;
      file_idx: number;
      watched: boolean;
    }
  | {
      status: "available";
      language: string | null;
      indexer_provider: string;
      indexer_torrent_id: string;
      quality: string | null;
      seeders: number | null;
      size_bytes: number | null;
    };

function mergeEpisodes(
  on_disk: CollectionEpisodeEntry[],
  available: AvailableEpisodeEntry[] | undefined,
): MergedEpisode[] {
  // Server already filters owned (season, episode, language) out of
  // `available` — variants we get here are genuinely additive. We
  // group everything by (S, E) for a single row per episode.
  // episode === 0 is the season-pack sentinel — the file fallback
  // path handles those separately.
  const byKey = new Map<string, MergedEpisode>();
  const ensure = (season: number, episode: number): MergedEpisode => {
    const key = `${season}-${episode}`;
    let row = byKey.get(key);
    if (!row) {
      row = { season, episode, variants: [] };
      byKey.set(key, row);
    }
    return row;
  };
  for (const d of on_disk) {
    if (d.episode === 0) continue;
    ensure(d.season, d.episode).variants.push({
      status: "downloaded",
      language: d.language ?? null,
      infohash: d.infohash,
      file_idx: d.file_idx,
      watched: d.watched,
    });
  }
  for (const a of available ?? []) {
    if (a.episode === 0) continue;
    ensure(a.season, a.episode).variants.push({
      status: "available",
      language: a.language ?? null,
      indexer_provider: a.indexer_provider,
      indexer_torrent_id: a.indexer_torrent_id,
      quality: a.quality,
      seeders: a.seeders,
      size_bytes: a.size_bytes,
    });
  }
  // Stable variant order per row: downloaded first (so the Play
  // action sits to the left as the natural primary), then available
  // sorted by language for predictable adjacency.
  for (const row of byKey.values()) {
    row.variants.sort((a, b) => {
      if (a.status !== b.status) return a.status === "downloaded" ? -1 : 1;
      return (a.language ?? "").localeCompare(b.language ?? "");
    });
  }
  return Array.from(byKey.values()).sort(
    (a, b) => a.season - b.season || a.episode - b.episode,
  );
}

function EpisodeList({
  collection,
  onPlay,
}: {
  collection: CollectionDetail;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const episodes = useMemo(
    () => mergeEpisodes(collection.episodes, collection.available_episodes),
    [collection.episodes, collection.available_episodes],
  );

  // Pack offers cover whole seasons — surfaced separately because
  // the UX is "Grab full Season N", not a per-episode row. Grouped
  // by season for the banner above the episode list.
  const packsBySeason = useMemo(() => {
    const map = new Map<number, NonNullable<CollectionDetail["season_packs"]>>();
    for (const p of collection.season_packs ?? []) {
      const arr = map.get(p.season) ?? [];
      arr.push(p);
      map.set(p.season, arr);
    }
    return map;
  }, [collection.season_packs]);

  // Union of seasons we know about — from explicit episodes AND
  // from pack-only seasons (where the indexer cached a pack but
  // no singletons yet). Without that union, a brand-new follow
  // whose only signal is a pack would render an empty page.
  const seasons = useMemo(() => {
    const grouped = new Map<number, MergedEpisode[]>();
    for (const ep of episodes) {
      const arr = grouped.get(ep.season) ?? [];
      arr.push(ep);
      grouped.set(ep.season, arr);
    }
    for (const s of packsBySeason.keys()) {
      if (!grouped.has(s)) grouped.set(s, []);
    }
    return Array.from(grouped.entries())
      .sort(([a], [b]) => a - b)
      .map(([season, items]) => ({ season, items }));
  }, [episodes, packsBySeason]);

  const [activeSeason, setActiveSeason] = useState<number | null>(null);
  useEffect(() => {
    if (activeSeason == null && seasons.length > 0) {
      setActiveSeason(seasons[0].season);
    }
  }, [activeSeason, seasons]);

  if (seasons.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No episodes parsed yet for this collection.
      </p>
    );
  }

  const current = seasons.find((s) => s.season === activeSeason) ?? seasons[0];
  const currentPacks = packsBySeason.get(current.season) ?? [];

  return (
    <div className="grid gap-4">
      <SeasonTabs
        seasons={seasons.map((s) => s.season)}
        value={current.season}
        onChange={setActiveSeason}
      />
      {currentPacks.map((p) => (
        <SeasonPackBanner
          key={`${p.season}-${p.language ?? "_"}-${p.indexer_torrent_id}`}
          collectionId={collection.id}
          pack={p}
          onPlay={onPlay}
        />
      ))}
      {current.items.length > 0 ? (
        <ul className="divide-y divide-border rounded-lg border border-border bg-card/30">
          {current.items.map((ep) => (
            <EpisodeRow
              key={`${ep.season}-${ep.episode}`}
              collectionId={collection.id}
              ep={ep}
              onPlay={onPlay}
            />
          ))}
        </ul>
      ) : (
        // Season exists only because a pack covers it — no
        // singletons / on-disk episodes yet. The pack banner above
        // is the user's only action; this caption explains why.
        <p className="text-sm text-muted-foreground">
          No individual episode releases cached. Grab the season pack
          above to pull every episode in one go.
        </p>
      )}
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

function SeasonPackBanner({
  collectionId,
  pack,
  onPlay,
}: {
  collectionId: string;
  pack: NonNullable<CollectionDetail["season_packs"]>[number];
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const qc = useQueryClient();
  // Grabbing the pack ingests the whole season; the backend
  // resolves us to S0XE01 inside the pack so playback can start
  // right away. Episode 1 is a deliberate choice — once
  // collection_assign processes the pack on ingest, episode_files
  // rows materialise for every leaf and subsequent visits see the
  // whole season as "downloaded".
  const grab = useMutation({
    mutationFn: () =>
      library.grabCollectionEpisode(collectionId, pack.season, 1, pack.language ?? null),
    onSuccess: (res) => {
      void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
      onPlay(res.infohash, res.file_idx);
    },
  });
  const prepare = useMutation({
    mutationFn: () =>
      library.grabCollectionEpisode(collectionId, pack.season, 1, pack.language ?? null),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
    },
  });
  const busy = grab.isPending || prepare.isPending;
  return (
    <div className="grid grid-cols-[1fr_auto] items-center gap-3 rounded-lg border border-emerald-500/40 bg-gradient-to-r from-emerald-500/15 via-emerald-500/5 to-transparent px-4 py-3">
      <div className="min-w-0 grid gap-1">
        <div className="flex items-center gap-2">
          <Badge className="bg-emerald-500/80 text-[10px] uppercase">Season pack</Badge>
          <span className="text-sm font-medium">
            Season {pack.season} · full pack available
          </span>
          <LanguageBadge language={pack.language} />
        </div>
        <div className="text-xs text-muted-foreground">
          {[
            pack.quality,
            pack.seeders != null ? `${pack.seeders} seeders` : null,
            pack.size_bytes != null ? formatSize(pack.size_bytes) : null,
            `via ${pack.indexer_provider}`,
          ]
            .filter(Boolean)
            .join(" · ")}
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Button size="sm" variant="secondary" onClick={() => prepare.mutate()} disabled={busy}>
          <Download className="size-3.5" />
          Prepare
        </Button>
        <Button size="sm" onClick={() => grab.mutate()} disabled={busy}>
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
          Grab & play
        </Button>
      </div>
    </div>
  );
}

function EpisodeRow({
  collectionId,
  ep,
  onPlay,
}: {
  collectionId: string;
  ep: MergedEpisode;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const anyWatched = ep.variants.some(
    (v) => v.status === "downloaded" && v.watched,
  );
  return (
    <li className="grid grid-cols-[3rem_1fr] items-start gap-3 px-4 py-3 text-sm">
      <span className="pt-1 text-center font-mono text-muted-foreground">
        {ep.episode.toString().padStart(2, "0")}
      </span>
      <div className="min-w-0 grid gap-2">
        <div className="flex items-center gap-2">
          <span className="font-medium">
            S{ep.season.toString().padStart(2, "0")}E
            {ep.episode.toString().padStart(2, "0")}
          </span>
          {anyWatched && (
            <Badge variant="secondary" className="text-[10px]">
              <CheckCircle2 className="mr-1 size-3" /> watched
            </Badge>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {ep.variants.map((v, i) => (
            <VariantChip
              key={i}
              collectionId={collectionId}
              season={ep.season}
              episode={ep.episode}
              variant={v}
              onPlay={onPlay}
            />
          ))}
        </div>
      </div>
    </li>
  );
}

/** One clickable chip per release variant. Downloaded variants play
 *  immediately; available variants grab in the chip's language then
 *  play. Layout intentionally compact — a 4-variant row (rare but
 *  possible: FR + VOSTFR + EN + MULTi) still fits on a single line
 *  on a desktop. */
function VariantChip({
  collectionId,
  season,
  episode,
  variant,
  onPlay,
}: {
  collectionId: string;
  season: number;
  episode: number;
  variant: EpisodeVariant;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const qc = useQueryClient();
  const grab = useMutation({
    mutationFn: () =>
      library.grabCollectionEpisode(
        collectionId,
        season,
        episode,
        variant.status === "available" ? variant.language : null,
      ),
    onSuccess: (res) => {
      void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
      onPlay(res.infohash, res.file_idx);
    },
  });

  if (variant.status === "downloaded") {
    return (
      <button
        type="button"
        onClick={() => onPlay(variant.infohash, variant.file_idx)}
        className="group inline-flex items-center gap-2 rounded-md border border-border bg-card/60 px-2.5 py-1 transition hover:border-primary/60 hover:bg-card"
      >
        <LanguageBadge language={variant.language} />
        <Play className="size-3.5 text-primary" />
        <span className="text-xs text-muted-foreground">
          {variant.watched ? "Watch again" : "Play"}
        </span>
      </button>
    );
  }
  const meta = [
    variant.quality,
    variant.seeders != null ? `${variant.seeders}↑` : null,
    variant.size_bytes != null ? formatSize(variant.size_bytes) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <button
      type="button"
      onClick={() => grab.mutate()}
      disabled={grab.isPending}
      className="group inline-flex items-center gap-2 rounded-md border border-emerald-500/30 bg-emerald-500/5 px-2.5 py-1 transition hover:border-emerald-500/60 hover:bg-emerald-500/10 disabled:opacity-60"
    >
      <LanguageBadge language={variant.language} />
      {grab.isPending ? (
        <Loader2 className="size-3.5 animate-spin text-emerald-300" />
      ) : (
        <Download className="size-3.5 text-emerald-300" />
      )}
      {meta && <span className="text-xs text-muted-foreground">{meta}</span>}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Raw file fallback — for movies with no SCENE-parseable video, and
// any other long-tail edge case the merged episode list can't render.
// ---------------------------------------------------------------------------

function RawFileList({
  collection,
  onPlay,
}: {
  collection: CollectionDetail;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  return (
    <ul className="divide-y divide-border rounded-lg border border-border bg-card/30">
      {collection.torrents.flatMap((t) =>
        // Server already returns files in SCENE-aware order
        // (`compare_video_files` inside the engine snapshot
        // builder) — don't second-guess that on the client.
        t.files
          .filter((f) => VIDEO_RE.test(f.path))
          .map((f) => (
            <li
              key={`${t.infohash}:${f.index}`}
              className="flex items-center justify-between gap-3 px-4 py-3 text-sm"
            >
              <span className="truncate font-mono text-xs text-muted-foreground">
                {f.path.split("/").pop()}
              </span>
              <Button size="sm" className="ml-auto shrink-0" onClick={() => onPlay(t.infohash, f.index)}>
                <Play className="size-3.5" />
                Play
              </Button>
            </li>
          )),
      )}
    </ul>
  );
}
