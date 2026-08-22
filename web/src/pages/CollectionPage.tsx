import { useEffect, useMemo, useState } from "react";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  Download,
  Film,
  Loader2,
  Play,
  RotateCcw,
  Sparkles,
  Trash2,
  Tv,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Container } from "@/components/Container";
import { LanguageBadge } from "@/components/LanguageBadge";
import { Tag } from "@/components/Tag";
import {
  library,
  me,
  tmdbImage,
  torrents,
  type AvailableEpisodeEntry,
  type CollectionDetail,
  type CollectionEpisodeEntry,
  type GoneEpisodeEntry,
  type GoneReleaseEntry,
} from "@/lib/api";
import { formatRecentTime, formatSize, formatTimecode } from "@/lib/format";
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
const collectionRoute = getRouteApi("/auth/shell/collection/$id");

export function CollectionPage() {
  const { id } = collectionRoute.useParams();
  const navigate = useNavigate();
  const { data, isLoading, error } = useQuery({
    queryKey: ["collection", id],
    queryFn: () => library.collection(id),
    enabled: !!id,
  });

  // Auto-navigate movies straight to /watch when there's a single
  // playable file. Saves a useless intermediate click. TV stays on
  // this page even when only one episode is on disk — the new
  // "grab next episode" surface is the point of stopping here.
  // Multi-copy movies stay too: the page is the only surface where
  // the user can see the copies, pick one, or delete the extras —
  // auto-picking `torrents[0]` hid every other copy.
  useEffect(() => {
    if (!data) return;
    if (data.kind === "movie" && data.torrents.length === 1) {
      const t = data.torrents[0];
      const f = t?.files
        .filter((x) => VIDEO_RE.test(x.path))
        .sort((a, b) => b.size_bytes - a.size_bytes)[0];
      if (t && f) {
        navigate({
          to: "/watch/$infohash/$idx",
          params: { infohash: t.infohash, idx: String(f.index) },
          replace: true,
        });
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

  // play whatever is on disk. Gone episodes count too: a ghost TV
  // collection must render its episode list, not the raw picker.
  const tvHasEpisodes =
    data.kind === "tv" &&
    (data.episodes.length > 0 ||
      (data.available_episodes?.length ?? 0) > 0 ||
      (data.gone_episodes?.length ?? 0) > 0);

  // Releases whose gone episodes render inline don't need a second
  // row below; episode === 0 (pack sentinel) rows keep theirs.
  const goneInline = new Set(
    (data.gone_episodes ?? []).filter((g) => g.episode > 0).map((g) => g.infohash),
  );
  const rawGoneReleases = (data.gone_releases ?? []).filter((r) => !goneInline.has(r.infohash));

  return (
    <div>
      <Hero collection={data} />
      <Container>
        {tvHasEpisodes ? (
          <EpisodeList
            collection={data}
            onPlay={(ih, idx) =>
              navigate({ to: "/watch/$infohash/$idx", params: { infohash: ih, idx: String(idx) } })
            }
          />
        ) : data.kind === "movie" && data.torrents.length > 1 ? (
          // Several copies of the same movie: one card per release so
          // the user can tell them apart, play any of them, and delete
          // the surplus. The flat file list hid this entirely.
          <ReleaseVersions
            collection={data}
            collectionId={id}
            onPlay={(ih, idx) =>
              navigate({ to: "/watch/$infohash/$idx", params: { infohash: ih, idx: String(idx) } })
            }
          />
        ) : (
          // Single-copy movies without a playable file, or a TV pack the
          // SCENE parser couldn't break into episodes. Either way the
          // raw file picker gets the user unblocked.
          <RawFileList
            collection={data}
            onPlay={(ih, idx) =>
              navigate({ to: "/watch/$infohash/$idx", params: { infohash: ih, idx: String(idx) } })
            }
          />
        )}
        {rawGoneReleases.length > 0 && (
          <GoneReleases collectionId={id} releases={rawGoneReleases} />
        )}
      </Container>
    </div>
  );
}

/** History-style status line for a gone release. */
function goneWatchLine(r: GoneReleaseEntry): string | null {
  if (r.watched) {
    return r.last_watched_at ? `Watched · ${formatRecentTime(r.last_watched_at)}` : "Watched";
  }
  if (r.position_seconds == null || r.position_seconds <= 0) return null;
  const pct =
    r.duration_seconds && r.duration_seconds > 0
      ? Math.min(100, (r.position_seconds / r.duration_seconds) * 100)
      : null;
  const parts = [
    pct != null ? `${pct.toFixed(0)}%` : null,
    formatTimecode(r.position_seconds),
    r.last_watched_at ? formatRecentTime(r.last_watched_at) : null,
  ].filter(Boolean);
  return parts.join(" · ");
}

/**
 * "Previously on disk" — what the episode list can't show inline:
 * movies and packs the parser never split. Watch state first, SCENE
 * name second; Download again re-ingests (same infohash, saved
 * position resumes) and the × is a per-user hide.
 */
function GoneReleases({
  collectionId,
  releases,
}: {
  collectionId: string;
  releases: GoneReleaseEntry[];
}) {
  const qc = useQueryClient();
  const [busyInfohash, setBusyInfohash] = useState<string | null>(null);
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
    void qc.invalidateQueries({ queryKey: ["library"] });
    void qc.invalidateQueries({ queryKey: ["history"] });
  };
  const regrab = useMutation({
    // Ghost-resume of a specific reclaimed release — explicit intent,
    // skip the duplicate-movie guard.
    mutationFn: (r: GoneReleaseEntry) =>
      torrents.ingest(r.source_provider, r.source_external_id, null, true),
    onSuccess: refresh,
    onSettled: () => setBusyInfohash(null),
  });
  const dismiss = useMutation({
    mutationFn: (r: GoneReleaseEntry) => me.dismissGone({ infohash: r.infohash }),
    onSuccess: refresh,
  });

  return (
    <section className="glass mt-8 grid gap-3 rounded-xl p-4">
      <span className="eyebrow">Previously on disk ({releases.length})</span>
      <ul className="grid gap-1">
        {releases.map((r) => {
          const busy = busyInfohash === r.infohash;
          const watchLine = goneWatchLine(r);
          return (
            <li key={r.infohash} className="flex items-center gap-3 rounded-lg px-2.5 py-2 text-sm">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  {r.watched && (
                    <Tag variant="plain" upper>
                      <CheckCircle2 className="size-2.5" /> Watched
                    </Tag>
                  )}
                  {watchLine && !r.watched && (
                    <span className="text-xs text-muted-foreground">{watchLine}</span>
                  )}
                  {r.watched && r.last_watched_at && (
                    <span className="text-xs text-muted-foreground">
                      {formatRecentTime(r.last_watched_at)}
                    </span>
                  )}
                </div>
                <div className="truncate font-mono text-xs" title={r.name}>
                  {r.name}
                </div>
                <div className="text-xs text-muted-foreground">
                  {formatSize(r.total_size_bytes)} · via {r.source_provider}
                  {r.deleted_at ? ` · removed ${formatRecentTime(r.deleted_at)}` : ""}
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => {
                  setBusyInfohash(r.infohash);
                  regrab.mutate(r);
                }}
                title="Re-download this exact release. Your watch position is kept"
              >
                {busy ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Download className="size-4" />
                )}
                {busy ? "Restoring…" : "Download again"}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-8 shrink-0 text-muted-foreground hover:text-foreground"
                disabled={dismiss.isPending}
                onClick={() => dismiss.mutate(r)}
                title="Hide this entry for you only. Your history is kept"
                aria-label="Hide this entry"
              >
                <X className="size-4" />
              </Button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

// Hero

function Hero({ collection }: { collection: CollectionDetail }) {
  // Server-resolved poster + backdrop. Both `null` when the
  // collection has no tmdb_id or the TMDB lookup failed — the hero
  // falls back to a placeholder without rendering broken `<img>`.
  const poster = tmdbImage(collection.poster_path, "w500");
  const backdrop = tmdbImage(collection.backdrop_path, "original");
  const newCount = collection.has_new_since_last_visit ?? 0;

  // The episode to start from when there's NO resume point. Pick the first
  // ON-DISK episode by (season, episode) — NOT `files[0]`, which on a season
  // pack is often a sample / extra / out-of-order file and sent "Play" to the
  // wrong episode. `episodes` carries the SCENE-parsed (S,E) → (infohash,
  // file_idx) mapping; we mirror EpisodeList's sort. `episode === 0` is the
  // season-pack sentinel — skip it in favour of real episodes.
  const firstPlayable = useMemo(() => {
    const owned = collection.episodes
      .filter((e) => e.episode > 0)
      .sort((a, b) => a.season - b.season || a.episode - b.episode);
    if (owned.length > 0) {
      return { infohash: owned[0].infohash, idx: owned[0].file_idx };
    }
    // Fallback (movie, or a pack the SCENE parser couldn't split into
    // episodes): first video file on disk.
    for (const t of collection.torrents) {
      const f = t.files.find((x) => VIDEO_RE.test(x.path));
      if (f) return { infohash: t.infohash, idx: f.index };
    }
    return null;
  }, [collection.episodes, collection.torrents]);

  // Resume point: the most-recently-watched IN-PROGRESS episode of THIS
  // collection (continue-watching only carries incomplete items). When present
  // the CTA resumes that episode rather than restarting at episode 1.
  const cw = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });
  const resume = useMemo(() => {
    if (!cw.data) return null;
    const owned = new Set(collection.torrents.map((t) => t.infohash));
    const here = cw.data
      .filter((it) => !it.completed && owned.has(it.infohash))
      .sort((a, b) => Date.parse(b.last_watched_at) - Date.parse(a.last_watched_at));
    const top = here[0];
    return top ? { infohash: top.infohash, idx: top.file_idx } : null;
  }, [cw.data, collection.torrents]);

  // Resume wins when we have one; otherwise start from the first episode.
  const playTarget = resume ?? firstPlayable;

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
            <h1
              className="display text-foreground [overflow-wrap:anywhere]"
              style={{ fontSize: "clamp(40px, 7vw, 76px)" }}
            >
              {collection.display_title}
            </h1>
            {playTarget && (
              <div className="flex flex-wrap gap-2.5">
                <Button
                  size="lg"
                  className="h-11"
                  render={
                    <Link
                      to="/watch/$infohash/$idx"
                      params={{ infohash: playTarget.infohash, idx: String(playTarget.idx) }}
                    />
                  }
                >
                  <Play className="size-4.5" />
                  {resume ? "Resume" : "Play"}
                </Button>
              </div>
            )}
          </div>
        </div>
      </Container>
    </section>
  );
}

// Episode list — merged on-disk + indexer offers

/** A single episode row aggregates every variant we have for that
 *  (S, E) — owned releases + grabbable indexer offers — so a user
 *  with the FR release on disk and an EN release available in cache
 *  sees one row with TWO chips, not two separate rows. The chip the
 *  user clicks decides whether we Play (downloaded) or Grab
 *  (available, with the right language). */
type MergedEpisode = {
  season: number;
  episode: number;
  /** Absolute episode number for fleuve anime — set only in the
   *  absolute-numbering layout. When present the row renders as
   *  "Episode N" instead of SxxExx. `season`/`episode` still carry the
   *  fansub coordinate (`S01`/absolute) used for the grab call. */
  absolute?: number | null;
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
    }
  | {
      // Reclaimed: renders in place with "Re-grab" instead of Play
      // (same infohash, saved position resumes). Per-user dismissable.
      status: "gone";
      language: string | null;
      infohash: string;
      file_idx: number;
      watched: boolean;
      release_name: string;
      quality: string | null;
      total_size_bytes: number;
      source_provider: string;
      source_external_id: string;
    };

/** A same-language (or MULTi) release on disk makes "Re-grab"
 *  pure noise — Play sits right next to it. */
function pruneShadowedGone(variants: EpisodeVariant[]): EpisodeVariant[] {
  const downloadedLangs = new Set(
    variants.filter((v) => v.status === "downloaded").map((v) => v.language ?? ""),
  );
  if (downloadedLangs.size === 0) return variants;
  const hasMulti = downloadedLangs.has("multi");
  return variants.filter(
    (v) => v.status !== "gone" || (!hasMulti && !downloadedLangs.has(v.language ?? "")),
  );
}

/** Downloaded, then gone (it WAS on disk), then available. */
const VARIANT_RANK = { downloaded: 0, gone: 1, available: 2 } as const;

function sortVariants(variants: EpisodeVariant[]) {
  variants.sort((a, b) => {
    if (a.status !== b.status) return VARIANT_RANK[a.status] - VARIANT_RANK[b.status];
    return (a.language ?? "").localeCompare(b.language ?? "");
  });
}

function goneVariant(g: GoneEpisodeEntry): EpisodeVariant {
  return {
    status: "gone",
    language: g.language ?? null,
    infohash: g.infohash,
    file_idx: g.file_idx,
    watched: g.watched,
    release_name: g.release_name,
    quality: g.quality ?? null,
    total_size_bytes: g.total_size_bytes ?? 0,
    source_provider: g.source_provider,
    source_external_id: g.source_external_id,
  };
}

function mergeEpisodes(
  on_disk: CollectionEpisodeEntry[],
  available: AvailableEpisodeEntry[] | undefined,
  gone: GoneEpisodeEntry[] | undefined,
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
  // Gone episodes keep their (S, E) slot.
  for (const g of gone ?? []) {
    if (g.episode === 0) continue;
    ensure(g.season, g.episode).variants.push(goneVariant(g));
  }
  for (const a of available ?? []) {
    if (a.episode === 0) continue;
    ensure(a.season, a.episode).variants.push({
      status: "available",
      language: a.language ?? null,
      indexer_provider: a.indexer_provider,
      indexer_torrent_id: a.indexer_torrent_id,
      quality: a.quality ?? null,
      seeders: a.seeders ?? null,
      size_bytes: a.size_bytes ?? null,
    });
  }
  for (const row of byKey.values()) {
    row.variants = pruneShadowedGone(row.variants);
    sortVariants(row.variants);
  }
  return Array.from(byKey.values()).sort((a, b) => a.season - b.season || a.episode - b.episode);
}

/** Absolute-numbering merge for fleuve anime (One Piece): one flat list
 *  keyed on the *absolute* episode number, no seasons.
 *
 *  A long-running anime is released two ways at once — the fleuve
 *  fansubs (`S01E1156`, absolute number known) AND season-cut releases
 *  (`S23E07`, no derivable absolute). The season-cut ones have no valid
 *  position on the absolute axis, so we MUST NOT fold them in under
 *  their raw `episode` — that's what made unrelated cuts show up as a
 *  bogus "Episode 1..7". Rule:
 *    - owned (downloaded) episodes always appear (never hide what's on
 *      disk) — by absolute when known, else by their `SxxExx` coord;
 *    - available offers appear only when they carry an absolute number;
 *      season-cut offers are left out of this axis (still in the DB,
 *      surfaced if the collection is ever shown seasonally).
 *  `absolute` drives the "Episode N" label; rows keep (season, episode)
 *  for the grab call. */
function mergeEpisodesAbsolute(
  on_disk: CollectionEpisodeEntry[],
  available: AvailableEpisodeEntry[] | undefined,
  gone: GoneEpisodeEntry[] | undefined,
): MergedEpisode[] {
  const byKey = new Map<string, MergedEpisode>();
  const ensure = (abs: number | null, season: number, episode: number): MergedEpisode => {
    const key = abs != null ? `a:${abs}` : `s:${season}:${episode}`;
    let row = byKey.get(key);
    if (!row) {
      row = { season, episode, absolute: abs, variants: [] };
      byKey.set(key, row);
    }
    return row;
  };
  for (const d of on_disk) {
    if (d.episode === 0) continue;
    ensure(d.absolute_episode ?? null, d.season, d.episode).variants.push({
      status: "downloaded",
      language: d.language ?? null,
      infohash: d.infohash,
      file_idx: d.file_idx,
      watched: d.watched,
    });
  }
  // Gone episodes follow the on-disk rule: always shown — by
  // absolute when known, else by their SxxExx coordinate.
  for (const g of gone ?? []) {
    if (g.episode === 0) continue;
    ensure(g.absolute_episode ?? null, g.season, g.episode).variants.push(goneVariant(g));
  }
  for (const a of available ?? []) {
    if (a.episode === 0) continue;
    // Skip season-cut offers with no absolute — they can't be placed on
    // the absolute axis and would otherwise alias onto low episode rows.
    if (a.absolute_episode == null) continue;
    ensure(a.absolute_episode, a.season, a.episode).variants.push({
      status: "available",
      language: a.language ?? null,
      indexer_provider: a.indexer_provider,
      indexer_torrent_id: a.indexer_torrent_id,
      quality: a.quality ?? null,
      seeders: a.seeders ?? null,
      size_bytes: a.size_bytes ?? null,
    });
  }
  for (const row of byKey.values()) {
    row.variants = pruneShadowedGone(row.variants);
    sortVariants(row.variants);
  }
  // Absolute-numbered rows first (ascending); any owned-without-absolute
  // rows trail, ordered by their (season, episode).
  return Array.from(byKey.values()).sort((a, b) => {
    if (a.absolute != null && b.absolute != null) return a.absolute - b.absolute;
    if (a.absolute != null) return -1;
    if (b.absolute != null) return 1;
    return a.season - b.season || a.episode - b.episode;
  });
}

function EpisodeList({
  collection,
  onPlay,
}: {
  collection: CollectionDetail;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const episodes = useMemo(
    () =>
      mergeEpisodes(collection.episodes, collection.available_episodes, collection.gone_episodes),
    [collection.episodes, collection.available_episodes, collection.gone_episodes],
  );

  // Fleuve anime (One Piece): one flat absolute-numbered list, no
  // season tabs. The server derives this from the episode data, so a
  // season-cut anime still renders the seasonal layout below.
  const isAbsolute = collection.numbering === "absolute";
  const absoluteEpisodes = useMemo(
    () =>
      mergeEpisodesAbsolute(
        collection.episodes,
        collection.available_episodes,
        collection.gone_episodes,
      ),
    [collection.episodes, collection.available_episodes, collection.gone_episodes],
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
      // Season 0 is "Specials" — land on the first REAL season by default
      // so a show with OAVs/specials doesn't open on the specials tab.
      // Falls back to season 0 only when it's the only season available.
      const firstReal = seasons.find((s) => s.season > 0);
      setActiveSeason((firstReal ?? seasons[0]).season);
    }
  }, [activeSeason, seasons]);

  if (isAbsolute) {
    if (absoluteEpisodes.length === 0) {
      return (
        <p className="text-sm text-muted-foreground">No episodes parsed yet for this collection.</p>
      );
    }
    const downloadedCount = absoluteEpisodes.filter((e) =>
      e.variants.some((v) => v.status === "downloaded"),
    ).length;
    return (
      <div className="grid gap-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <span className="text-[13px] text-muted-foreground">
            {absoluteEpisodes.length} episodes · {downloadedCount} downloaded
          </span>
        </div>
        <ul className="grid gap-2">
          {absoluteEpisodes.map((ep) => (
            <EpisodeRow
              key={ep.absolute ?? ep.episode}
              collectionId={collection.id}
              ep={ep}
              onPlay={onPlay}
            />
          ))}
        </ul>
      </div>
    );
  }

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
    <div className="grid min-w-0 gap-6">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-4">
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
    // `min-w-0 flex-1 max-w-full` is what makes `overflow-x-auto` actually
    // engage: without a bounded width the flex item grows to its content
    // and a 20+ season strip pushes the whole page wide instead of
    // scrolling within the row.
    <div className="no-scrollbar -mx-1 flex min-w-0 max-w-full flex-1 gap-2 overflow-x-auto px-1 pb-1">
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
          {s === 0 ? "Specials" : `Season ${s}`}
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
  const anyWatched = ep.variants.some(
    (v) => (v.status === "downloaded" || v.status === "gone") && v.watched,
  );
  // Absolute (fleuve anime) rows show "Episode 1156"; seasonal rows
  // keep the SxxExx label. The big badge mirrors whichever number leads.
  const badgeNumber = ep.absolute ?? ep.episode;
  return (
    <li className="glass grid grid-cols-[auto_1fr] items-start gap-4 rounded-xl p-3.5 text-sm">
      <span className="grid size-11 place-items-center rounded-[10px] bg-elev-2 font-display text-lg text-foreground">
        {badgeNumber.toString().padStart(2, "0")}
      </span>
      <div className="min-w-0 grid gap-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[13px] text-muted-foreground">
            {ep.absolute != null
              ? `Episode ${ep.absolute}`
              : `S${ep.season.toString().padStart(2, "0")}E${ep.episode.toString().padStart(2, "0")}`}
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
  const refreshGone = () => {
    void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
    void qc.invalidateQueries({ queryKey: ["library"] });
    void qc.invalidateQueries({ queryKey: ["history"] });
  };
  // Gone chip: re-ingest the exact release, then play right away.
  const regrab = useMutation({
    mutationFn: () => {
      if (variant.status !== "gone") throw new Error("not a gone variant");
      // Same-release resurrect — explicit intent, skip the duplicate guard.
      return torrents.ingest(variant.source_provider, variant.source_external_id, null, true);
    },
    onSuccess: () => {
      refreshGone();
      if (variant.status === "gone") onPlay(variant.infohash, variant.file_idx);
    },
  });
  const dismissRelease = useMutation({
    mutationFn: () => {
      if (variant.status !== "gone") throw new Error("not a gone variant");
      return me.dismissGone({ infohash: variant.infohash });
    },
    onSuccess: refreshGone,
  });

  if (variant.status === "gone") {
    return (
      <span
        className="inline-flex items-center gap-2 rounded-full border border-dashed border-border bg-elev px-2.5 py-1"
        title={variant.release_name}
      >
        <LanguageBadge language={variant.language} />
        <button
          type="button"
          onClick={() => regrab.mutate()}
          disabled={regrab.isPending}
          className="inline-flex items-center gap-1.5 text-muted-foreground transition hover:text-foreground disabled:opacity-60"
        >
          {regrab.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RotateCcw className="size-3.5" />
          )}
          <span className="text-xs">
            {regrab.isPending
              ? "Restoring…"
              : [
                  "Re-grab",
                  variant.quality,
                  variant.total_size_bytes > 0 ? formatSize(variant.total_size_bytes) : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
          </span>
        </button>
        <button
          type="button"
          onClick={() => dismissRelease.mutate()}
          disabled={dismissRelease.isPending}
          className="text-fg-dim transition hover:text-foreground disabled:opacity-60"
          title="Hide this release for you only. Your history is kept"
          aria-label="Hide this release"
        >
          <X className="size-3" />
        </button>
      </span>
    );
  }

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

/**
 * Multi-copy movie view — one card per release so duplicates stay
 * visible and manageable. Play targets the release's main video file;
 * delete is a two-step inline confirm (no dialog) wired to
 * `DELETE /api/torrents/{infohash}`.
 */
function ReleaseVersions({
  collection,
  collectionId,
  onPlay,
}: {
  collection: CollectionDetail;
  collectionId: string;
  onPlay: (infohash: string, fileIdx: number) => void;
}) {
  const qc = useQueryClient();
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const del = useMutation({
    mutationFn: (infohash: string) => torrents.remove(infohash),
    onSuccess: () => {
      setConfirmDelete(null);
      void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
      void qc.invalidateQueries({ queryKey: ["library"] });
      void qc.invalidateQueries({ queryKey: ["history"] });
    },
  });
  return (
    <section className="grid gap-3">
      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
        {collection.torrents.length} copies on disk
      </div>
      <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
        {collection.torrents.map((t) => {
          const main = t.files
            .filter((f) => VIDEO_RE.test(f.path))
            .sort((a, b) => b.size_bytes - a.size_bytes)[0];
          const name = t.name ?? t.infohash;
          return (
            <li key={t.infohash} className="flex flex-wrap items-center gap-3 px-4 py-3 text-sm">
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-xs" title={name}>
                  {name}
                </div>
                <div className="mt-0.5 text-xs text-muted-foreground">
                  {formatSize(t.total_size_bytes)} · added by {t.added_by_name} ·{" "}
                  {formatRecentTime(t.added_at)}
                </div>
              </div>
              {confirmDelete === t.infohash ? (
                <>
                  <Button
                    size="sm"
                    variant="destructive"
                    disabled={del.isPending}
                    onClick={() => del.mutate(t.infohash)}
                  >
                    {del.isPending ? "Deleting…" : "Confirm delete"}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(null)}>
                    Keep
                  </Button>
                </>
              ) : (
                <>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-destructive hover:text-destructive"
                    title="Delete this copy"
                    onClick={() => setConfirmDelete(t.infohash)}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                  <Button
                    size="sm"
                    disabled={!main}
                    onClick={() => main && onPlay(t.infohash, main.index)}
                  >
                    <Play className="size-3.5" />
                    Play
                  </Button>
                </>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

// Raw file fallback — for movies with no SCENE-parseable video, and
// any other long-tail edge case the merged episode list can't render.

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
