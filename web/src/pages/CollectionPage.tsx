import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Download, Film, Loader2, Play, Sparkles, Tv } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Container } from "@/components/Container";
import { LanguageBadge } from "@/components/LanguageBadge";
import { Tag } from "@/components/Tag";
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
      <Container>
        <p className="flex items-center gap-2 py-10 text-sm text-muted-foreground">
          <Loader2 className="size-3 animate-spin" />
          Loading collection…
        </p>
      </Container>
    );
  }
  if (error) {
    return (
      <Container>
        <p className="py-10 text-sm text-destructive">
          {error instanceof Error ? error.message : "failed"}
        </p>
      </Container>
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
    data.kind === "tv" && (data.episodes.length > 0 || (data.available_episodes?.length ?? 0) > 0);

  return (
    <div>
      <Hero collection={data} />
      <Container>
        {tvHasEpisodes ? (
          <EpisodeList collection={data} onPlay={(ih, idx) => navigate(`/watch/${ih}/${idx}`)} />
        ) : (
          // Movies, or a TV pack the SCENE parser couldn't break into
          // episodes. Either way the raw file picker gets the user
          // unblocked.
          <RawFileList collection={data} onPlay={(ih, idx) => navigate(`/watch/${ih}/${idx}`)} />
        )}
      </Container>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------

function Hero({ collection }: { collection: CollectionDetail }) {
  // Server-resolved poster + backdrop. Both `null` when the
  // collection has no tmdb_id or the TMDB lookup failed — the hero
  // falls back to a placeholder without rendering broken `<img>`.
  const poster = tmdbImage(collection.poster_path, "w500");
  const backdrop = tmdbImage(collection.backdrop_path, "original");
  const newCount = collection.has_new_since_last_visit ?? 0;

  // First on-disk video → a real "Play / Continue" CTA when we have one.
  const firstPlayable = useMemo(() => {
    for (const t of collection.torrents) {
      const f = t.files.find((x) => VIDEO_RE.test(x.path));
      if (f) return { infohash: t.infohash, idx: f.index };
    }
    return null;
  }, [collection.torrents]);

  return (
    <section className="relative isolate mb-8">
      <div className="absolute inset-0 -z-10 overflow-hidden">
        {backdrop ? (
          <img src={backdrop} alt="" className="h-full w-full object-cover opacity-40" />
        ) : (
          <div className="poster-fallback h-full w-full" />
        )}
        <div
          className="absolute inset-0"
          style={{
            background: "linear-gradient(180deg, oklch(0 0 0 / 0.2) 0%, var(--background) 95%)",
          }}
        />
      </div>

      <Container>
        <div
          className="grid items-end gap-6 pb-8 sm:grid-cols-[auto_1fr] sm:gap-9"
          style={{ paddingTop: "clamp(32px, 6vw, 64px)" }}
        >
          <div className="relative aspect-2/3 w-[clamp(120px,28vw,200px)] shrink-0 overflow-hidden rounded-xl border border-border bg-elev shadow-2xl sm:w-[clamp(140px,18vw,220px)]">
            {poster ? (
              <img
                src={poster}
                alt={collection.display_title}
                className="h-full w-full object-cover"
              />
            ) : (
              <div className="poster-fallback grid h-full w-full place-items-center text-fg-dim">
                {collection.kind === "tv" ? <Tv className="size-9" /> : <Film className="size-9" />}
              </div>
            )}
          </div>

          <div className="grid min-w-0 gap-4">
            <div className="flex flex-wrap items-center gap-2">
              <Tag variant="accent" upper>
                {collection.kind === "tv" ? "Series" : "Movie"}
              </Tag>
              <span className="text-[13px] text-muted-foreground">
                {collection.torrents.length} torrent{collection.torrents.length > 1 ? "s" : ""}
              </span>
              {newCount > 0 && <Tag variant="success">{newCount} new</Tag>}
            </div>
            <h1 className="display text-foreground" style={{ fontSize: "clamp(40px, 7vw, 76px)" }}>
              {collection.display_title}
            </h1>
            {firstPlayable && (
              <div className="flex flex-wrap gap-2.5">
                <Button asChild size="lg" className="h-11">
                  <Link to={`/watch/${firstPlayable.infohash}/${firstPlayable.idx}`}>
                    <Play className="size-4.5" />
                    Play
                  </Link>
                </Button>
              </div>
            )}
          </div>
        </div>
      </Container>
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
  return Array.from(byKey.values()).sort((a, b) => a.season - b.season || a.episode - b.episode);
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
      <p className="text-sm text-muted-foreground">No episodes parsed yet for this collection.</p>
    );
  }

  const current = seasons.find((s) => s.season === activeSeason) ?? seasons[0];
  const currentPacks = packsBySeason.get(current.season) ?? [];
  const downloadedCount = current.items.filter((e) =>
    e.variants.some((v) => v.status === "downloaded"),
  ).length;

  return (
    <div className="grid gap-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <SeasonTabs
          seasons={seasons.map((s) => s.season)}
          value={current.season}
          onChange={setActiveSeason}
        />
        {current.items.length > 0 && (
          <span className="text-[13px] text-muted-foreground">
            {current.items.length} episodes · {downloadedCount} downloaded
          </span>
        )}
      </div>
      {currentPacks.map((p) => (
        <SeasonPackBanner
          key={`${p.season}-${p.language ?? "_"}-${p.indexer_torrent_id}`}
          collectionId={collection.id}
          pack={p}
          onPlay={onPlay}
        />
      ))}
      {current.items.length > 0 ? (
        <ul className="grid gap-2">
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
          No individual episode releases cached. Grab the season pack above to pull every episode in
          one go.
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
    <div className="no-scrollbar -mx-1 flex gap-2 overflow-x-auto px-1 pb-1">
      {seasons.map((s) => (
        <button
          key={s}
          type="button"
          onClick={() => onChange(s)}
          className={cn(
            "inline-flex h-9 items-center rounded-full border px-4 text-sm font-medium transition-colors",
            s === value
              ? "border-border bg-elev-2 text-foreground"
              : "border-transparent text-muted-foreground hover:bg-accent hover:text-foreground",
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
    <div className="grid grid-cols-1 items-center gap-3 rounded-xl border border-success/30 bg-gradient-to-r from-success/12 via-success/5 to-transparent px-4 py-3.5 sm:grid-cols-[1fr_auto]">
      <div className="min-w-0 grid gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <Tag variant="success" upper>
            <Sparkles className="size-2.5" /> Season pack
          </Tag>
          <span className="text-sm font-medium">Season {pack.season} · full pack available</span>
          <LanguageBadge language={pack.language} />
        </div>
        <div className="text-[12.5px] text-fg-dim">
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
  const anyWatched = ep.variants.some((v) => v.status === "downloaded" && v.watched);
  return (
    <li className="glass grid grid-cols-[auto_1fr] items-start gap-4 rounded-xl p-3.5 text-sm">
      <span className="grid size-11 place-items-center rounded-[10px] bg-elev-2 font-display text-lg text-foreground">
        {ep.episode.toString().padStart(2, "0")}
      </span>
      <div className="min-w-0 grid gap-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[13px] text-muted-foreground">
            S{ep.season.toString().padStart(2, "0")}E{ep.episode.toString().padStart(2, "0")}
          </span>
          {anyWatched && (
            <Tag variant="plain" upper>
              <CheckCircle2 className="size-2.5" /> Watched
            </Tag>
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
        className="group inline-flex items-center gap-2 rounded-full border border-border bg-elev-2 px-2.5 py-1 transition hover:border-primary/60 hover:bg-hover"
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
      className="group inline-flex items-center gap-2 rounded-full border border-success/30 bg-success/8 px-2.5 py-1 transition hover:border-success/60 hover:bg-success/15 disabled:opacity-60"
    >
      <LanguageBadge language={variant.language} />
      {grab.isPending ? (
        <Loader2 className="size-3.5 animate-spin text-success" />
      ) : (
        <Download className="size-3.5 text-success" />
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
    <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
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
              <Button
                size="sm"
                className="ml-auto shrink-0"
                onClick={() => onPlay(t.infohash, f.index)}
              >
                <Play className="size-3.5" />
                Play
              </Button>
            </li>
          )),
      )}
    </ul>
  );
}
