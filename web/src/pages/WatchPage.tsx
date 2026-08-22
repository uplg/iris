import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import {
  CheckCircle2,
  Download,
  Library as LibraryIcon,
  Loader2,
  Play,
  RectangleHorizontal,
} from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Container } from "@/components/Container";
import { NotFoundState } from "@/components/NotFoundState";
import { cn } from "@/lib/utils";
import {
  ApiError,
  follows,
  library,
  me as meApi,
  progress as progressApi,
  torrents,
  type AvailableEpisodeEntry,
  type FileEntry,
  type FileProgressEntry,
  type PlaybackPrefs,
  type PlayStatus,
  type TorrentView,
} from "@/lib/api";
import { formatSize } from "@/lib/format";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  fetchManifest,
  hlsUrl,
  ManifestNotReadyError,
  pickTier,
  postSeekHint,
  rawStreamUrl,
  type DecodeTier,
} from "@/lib/iris-core/manifest-client";
import { IrisPlayer } from "@/lib/iris-core/IrisPlayer";
import { readStoredVolume, writeStoredVolume } from "@/lib/player-volume";
import { readLocal, writeLocal } from "@/lib/safe-storage";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/** A title counts as watched/done once playback passes this fraction — the
 *  last ~10% is credits/recap, so 90% is "finished" for both movies (dropped
 *  from Continue Watching) and episodes (Continue Watching then advances to
 *  the next). Mirrors the Android TV player. */
const WATCHED_FRACTION = 0.9;
const isWatched = (t: number, dur: number | null): boolean =>
  dur != null && dur > 0 && t >= dur * WATCHED_FRACTION;

/** One row of the watch-page side panel — either an episode of the parent
 *  collection (possibly in a different torrent) or a file of the current
 *  torrent. Keyed on `(infohash, fileIdx)` since episodes can span
 *  torrents. */
type SideRow = {
  key: string;
  infohash: string;
  fileIdx: number;
  /** "S02E01" for collection episodes, the filename for raw files. */
  primary: string;
  /** Language tag (episodes) or file size (raw files). */
  secondary: string;
  mono: boolean;
  watched: boolean;
  watchedPct: number | null;
  active: boolean;
  /** Set for an episode Iris has discovered but not downloaded yet —
   *  `infohash`/`fileIdx` are empty/-1 placeholders. The row renders a
   *  "Grab & Play" button instead of Play; clicking it grabs the episode
   *  then navigates straight to it once ready. */
  grab?: { season: number; episode: number; language: string | null };
};

function watchedPctOf(p?: FileProgressEntry): number | null {
  return p && p.duration_seconds && p.duration_seconds > 0
    ? Math.min(100, (p.position_seconds / p.duration_seconds) * 100)
    : null;
}

// Theater mode (YouTube-style): full-width player, the episodes/files
// panel restacks below it (same order as the responsive layout).
// A device-level display choice like volume, so it's persisted locally
// and sticks across episodes + sessions.
const THEATER_KEY = "iris:theater";
/** How long before the end the auto-advance card appears. It doubles as the
 *  countdown, so this is also how long the user has to cancel. */
const AUTO_ADVANCE_LEAD_S = 20;

// Stable identity for the "collection not loaded" case: an inline `?? []` is a
// fresh array every render, which silently defeats the episode-list memo.
const NO_AVAILABLE_EPISODES: AvailableEpisodeEntry[] = [];

function readStoredTheater(): boolean {
  return readLocal(THEATER_KEY) === "1";
}

function writeStoredTheater(on: boolean): void {
  writeLocal(THEATER_KEY, on ? "1" : "0");
}

const watchRoute = getRouteApi("/auth/shell/watch/$infohash/$idx");

export function WatchPage() {
  const { infohash, idx } = watchRoute.useParams();
  const fileIdx = Number(idx);
  const navigate = useNavigate();
  const qc = useQueryClient();

  const [playerError, setPlayerError] = useState<string | null>(null);
  // Theater mode: the player spans the full viewport width and the
  // episodes/files panel restacks below it. Toggled by the header
  // button or the "t" key (YouTube muscle memory).
  const [theater, setTheater] = useState(readStoredTheater);
  const toggleTheater = useCallback(() => {
    setTheater((v) => {
      writeStoredTheater(!v);
      return !v;
    });
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "t" || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target;
      // Never steal the key from text entry.
      if (
        t instanceof HTMLInputElement ||
        t instanceof HTMLTextAreaElement ||
        t instanceof HTMLSelectElement ||
        (t instanceof HTMLElement && t.isContentEditable)
      ) {
        return;
      }
      e.preventDefault();
      toggleTheater();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleTheater]);
  const lastTimeRef = useRef(0);
  const lastSavedTimeRef = useRef(0);
  const lastDurationRef = useRef<number | null>(null);
  // Set on every user seek, consumed by the next progress save. The
  // server's reset guard refuses a near-zero position over substantial
  // stored progress UNLESS the save is flagged as a deliberate seek —
  // this is that flag (see `put_progress` in iris-api).
  const seekPendingRef = useRef(false);
  const progressLoadedRef = useRef(false);
  // "Watch next?" state — gated on a single-shot flip per
  // mount, plus a dismissal flag so user choosing "Later" doesn't
  // get re-prompted within the same session.
  const [nextEpModalOpen, setNextEpModalOpen] = useState(false);
  const [nextEpGrabbing, setNextEpGrabbing] = useState(false);
  const nextEpDismissedRef = useRef(false);
  const nextEpPromptedRef = useRef(false);
  // Auto-advance countdown, in whole seconds, or null when the card is
  // hidden. Fed from `timeupdate` rather than a timer: the number the user
  // reads IS the time left in the file, so there is nothing to tick.
  const [autoAdvanceIn, setAutoAdvanceIn] = useState<number | null>(null);
  const autoAdvanceOffRef = useRef(false);
  const subtitleTrackRef = useRef<number | null>(null);
  // Last user-picked audio track index (into `manifest.audio`). Kept
  // in a ref — not state — so the various save paths read the latest
  // value without re-rendering the player on each change.
  const audioTrackRef = useRef<number | null>(null);
  // Transient backend-outage handling. A 502/503/504 (e.g. a deploy
  // restarting the server) must NOT be mistaken for a codec failure and
  // demote the tier — see `handleEngineError`. `streamNonce` bumps to
  // force the engine to re-mount on the SAME tier once the backend is
  // back; `outageRef` mirrors `outage` so the async error handler reads
  // the latest value synchronously.
  const [outage, setOutage] = useState(false);
  const [streamNonce, setStreamNonce] = useState(0);
  const outageRef = useRef(false);

  const torrentQ = useQuery<TorrentView>({
    queryKey: ["torrent", infohash],
    queryFn: () => torrents.get(infohash),
    enabled: !!infohash,
    refetchInterval: 3000,
  });

  // Resurrect a GC-reclaimed release in place (dead-page "Grab it again"
  // button). Same provenance → same infohash → the URL stays valid and
  // saved positions apply; no navigation needed, the 3s `torrentQ` poll
  // picks the revived torrent up and the page comes alive on its own.
  const regrab = useMutation({
    mutationFn: () => torrents.regrab(infohash),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["torrent", infohash] });
      void qc.invalidateQueries({ queryKey: ["play-status", infohash, fileIdx] });
      void qc.invalidateQueries({ queryKey: ["probe", infohash, fileIdx] });
    },
  });

  const data = torrentQ.data;
  const file = useMemo(() => data?.files.find((f) => f.index === fileIdx), [data, fileIdx]);
  const videoFiles = useMemo<FileEntry[]>(
    () => (data?.files ?? []).filter((f) => VIDEO_RE.test(f.path)),
    [data],
  );

  // Cache-buster passed into `<IrisPlayer subtitleVersion>` for ASS/PGS
  // overlay URLs. Quantised to 5%-progress buckets — a fast 100 Mb/s
  // download crosses the whole bar in ~1 minute, so we want at most
  // ~20 worker-side re-fetches across that, not 100. The bump pin to
  // `"final"` once `finished` flips lets the server promote a permanent
  // cache (`.ok` sidecar) and HTTP-cache the response. The
  // SubtitleOverlay's URL effect calls `libass.setTrackByUrl` on every
  // bump — in-place re-fetch, no remount, no canvas flash, no menu
  // re-pick required. UX gap: subtitles trail the download by at most
  // ~5% of the file's duration — usually well past the playhead since
  // the video itself can't play past what's downloaded either.
  const subtitleVersion = useMemo(() => {
    if (!data) return "0";
    if (data.finished) return "final";
    return Math.floor(data.progress_pct / 5).toString();
  }, [data]);

  // Poll the playback-prep status until the .fmp4 cache is on disk and
  // ready to be served via byte-range. The status endpoint surfaces the
  // upstream torrent download progress and the in-flight ffmpeg remux so
  // we can render a meaningful loader instead of a generic spinner.
  const playStatusQ = useQuery({
    queryKey: ["play-status", infohash, fileIdx],
    queryFn: () => torrents.playStatus(infohash, fileIdx),
    enabled: !!infohash,
    refetchInterval: (q) => {
      const d = q.state.data;
      // Terminal server states stop the poll: the cache is ready, or prep
      // hit a sticky failure surfaced in `d.error`.
      if (d?.ready || d?.error) return false as const;
      // Otherwise keep polling at 1s — INCLUDING the window where there's
      // no data yet because the request itself is failing transiently. The
      // old `!d` guard parked the query permanently the moment the first
      // fetch erred out (after the 8-retry budget), wedging the Tier F
      // loading screen until a manual page refresh. Tier F is the only
      // path gated on /play/status, and Firefox forces all HEVC content to
      // Tier F — hence the random, Firefox-mostly "stuck loader".
      return 1000;
    },
    retry: 8,
    retryDelay: 2000,
  });
  const playReady = playStatusQ.data?.ready === true;

  // Do NOT gate the probe on the whole torrent finishing. Click-to-play
  // must work as soon as the head is on disk — a 4 K remux is tens of GB
  // and waiting for 100% (or watching it sit at 99%) defeats the point.
  // The backend `/probe` route now actively prefetches the container
  // header + tail and returns a retryable "file not yet on disk" 400
  // while those bytes are still arriving (instead of a 500 on ffprobe's
  // "EBML header parsing failed"). So we fire as soon as we have a
  // torrent record and poll on the not-ready signal, exactly like the
  // manifest query below.
  const probeQ = useQuery({
    queryKey: ["probe", infohash, fileIdx],
    queryFn: () => torrents.probe(infohash, fileIdx),
    enabled: !!infohash,
    retry: (failureCount, err) => {
      // The backend prefetch can take up to ~30s per attempt to pull the
      // head from a slow private-tracker swarm; keep polling on the
      // not-ready signal with the same budget as the manifest query.
      const msg = err instanceof Error ? err.message : "";
      return msg.includes("not yet on disk") && failureCount < 30;
    },
    retryDelay: 2000,
    // Safety net: the finite `retry` budget above gives up after ~60s. On
    // a slow swarm the head bytes can take longer than that to land, which
    // used to wedge the loader on the loading screen until a manual page
    // refresh reset the retry counter. Once the burst is exhausted React
    // Query parks the query in a terminal error with no auto-refetch, so
    // re-poll on the not-ready signal until the disk catches up.
    refetchInterval: (q) => {
      if (q.state.data) return false as const;
      const err = q.state.error;
      return err instanceof Error && err.message.includes("not yet on disk")
        ? 2000
        : (false as const);
    },
  });

  const probe = probeQ.data;

  // Tiered cascade entry. Drop the `downloadFinished` gate: the
  // manifest endpoint now handles partial downloads (Phase 1 tail
  // prefetch), so we should fire as soon as we have a torrent record.
  // The ManifestNotReadyError retry handles the early-download window.
  const manifestQ = useQuery({
    queryKey: ["manifest", infohash, fileIdx],
    queryFn: () => fetchManifest(infohash, fileIdx),
    enabled: !!infohash,
    retry: (failureCount, err) => err instanceof ManifestNotReadyError && failureCount < 30,
    retryDelay: 2000,
    // Same safety net as `probeQ`: don't let the query die permanently when
    // the head bytes outrun the ~60s retry budget — re-poll on the not-ready
    // signal so the player self-heals instead of needing a page refresh.
    refetchInterval: (q) =>
      q.state.data
        ? (false as const)
        : q.state.error instanceof ManifestNotReadyError
          ? 2000
          : (false as const),
  });
  const manifest = manifestQ.data;
  const [tier, setTier] = useState<DecodeTier>("F");
  // Tiers we've already demoted away from this mount. Subsequent
  // pickTier results that name them collapse straight to F.
  const demotedRef = useRef<Set<DecodeTier>>(new Set());
  // True while a demote is in flight — swallows redundant onError
  // callbacks fired by the dying engine before its dispose unwinds.
  const demotionInProgressRef = useRef<DecodeTier | null>(null);
  useEffect(() => {
    if (!manifest) return;
    // Debug override: `?tier=F` (or A/B/C/D/E) on the URL pins the
    // engine instead of letting `pickTier` decide. Useful for
    // verifying a specific code path (e.g. server-side DV strip on
    // Tier F) without having to coax the cascade into demoting.
    const forced = new URLSearchParams(window.location.search).get("tier");
    if (forced && /^[A-F]$/i.test(forced)) {
      const t = forced.toUpperCase() as DecodeTier;
      setTier(t);
      console.log("[iris-core] tier", t, "(forced via ?tier=)");
      return;
    }
    void pickTier(manifest).then((t) => {
      const final = demotedRef.current.has(t) ? "F" : t;
      setTier(final);
      console.log("[iris-core] tier", final, {
        container: manifest.container,
        video: manifest.video.map((v) => v.codec_string ?? v.codec),
        audio: manifest.audio.map((a) => `${a.codec}${a.browser_native ? "(native)" : ""}`),
      });
    });
  }, [manifest]);

  /** Decide the next tier after a failure. Tier C/D failing on HEVC
   *  in a Chromium-family ≤ 1080p browser is routed through Tier E
   *  (hevc.js WASM transcode) before falling back to server-side
   *  HLS — the user's bandwidth + Chrome's HEVC HW story makes E
   *  the correct stop on the way down. */
  const nextDemotionTarget = useCallback(
    (from: DecodeTier): DecodeTier => {
      if ((from === "C" || from === "D") && manifest) {
        const primary = manifest.video[0];
        const isHevc = primary != null && /hevc|hev1|hvc1|h265|x265/i.test(primary.codec);
        const within1080p = (primary?.height ?? 0) <= 1080 && (primary?.height ?? 0) > 0;
        const chromiumish =
          /Chrome|Edg/.test(navigator.userAgent) && !/Mobile/.test(navigator.userAgent);
        if (isHevc && within1080p && chromiumish && !demotedRef.current.has("E")) {
          return "E";
        }
      }
      return "F";
    },
    [manifest],
  );

  const demoteTier = useCallback(
    (from: DecodeTier, reason: string) => {
      // Debounce: if we already demoted this tier in this mount,
      // swallow follow-on errors fired during teardown.
      if (demotedRef.current.has(from)) return;
      if (demotionInProgressRef.current === from) return;
      demotionInProgressRef.current = from;
      const target = nextDemotionTarget(from);
      console.warn(`[iris-core] tier ${from} → ${target} (${reason})`);
      demotedRef.current.add(from);
      setPlayerError(null);
      setTier(target);
      // Clear the in-flight guard on the next tick — by then the
      // engine has unmounted and any stragglers are harmless.
      setTimeout(() => {
        demotionInProgressRef.current = null;
      }, 250);
      if (manifest) {
        void fetch(`/api/torrents/${manifest.infohash}/files/${manifest.file_idx}/playback-error`, {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            tier: from,
            reason,
            codec: manifest.video[0]?.codec ?? null,
            browser: navigator.userAgent,
          }),
          keepalive: true,
        }).catch(() => undefined);
      }
    },
    [manifest, nextDemotionTarget],
  );

  // Probe whether the backend is reachable right now. A HEAD to the raw
  // stream endpoint goes through the same reverse proxy as everything
  // else, so a deploy/restart surfaces as 502/503/504 (or a network
  // throw). Any status < 500 means the server is up — even 404/405/416.
  const backendReachable = useCallback(async (): Promise<boolean> => {
    if (!infohash) return false;
    try {
      const res = await fetch(rawStreamUrl(infohash, fileIdx), {
        method: "HEAD",
        credentials: "include",
      });
      return res.status < 500;
    } catch {
      return false;
    }
  }, [infohash, fileIdx]);

  // Engine error router. A 502/503/504 or unreachable backend mid-
  // playback — e.g. the user redeploying the server — is NOT a codec
  // failure: demoting to F is useless (the server is down too) and
  // sticky (we'd stay on the worse tier after recovery). Probe first;
  // on an outage, hold the current tier and let `recoveryQ` re-mount it
  // when the backend returns. Only genuine engine errors (backend up)
  // demote (B/C/D/E) or surface the banner (A/F).
  const handleEngineError = useCallback(
    async (from: DecodeTier, msg: string) => {
      if (outageRef.current) return; // already reconnecting
      if (!(await backendReachable())) {
        console.warn(
          `[iris-core] tier ${from}: backend unreachable (${msg}) — holding tier, reconnecting`,
        );
        outageRef.current = true;
        setOutage(true);
        return;
      }
      if (from === "A" || from === "F") setPlayerError(msg);
      else demoteTier(from, msg);
    },
    [backendReachable, demoteTier],
  );

  // While an outage is active, poll the backend; when it answers again,
  // clear the outage and bump `streamNonce` to re-mount the engine on
  // the same tier (the `src` change is what re-triggers IrisPlayer's
  // mount effect). The `streamNonce` in the key gives each outage cycle
  // a fresh query so a later outage can't read a stale `true`.
  const recoveryQ = useQuery({
    queryKey: ["backend-recovery", infohash, fileIdx, streamNonce],
    queryFn: () => backendReachable(),
    enabled: outage,
    refetchInterval: (q) => (q.state.data === true ? (false as const) : 2000),
    gcTime: 0,
  });
  useEffect(() => {
    if (outage && recoveryQ.data === true) {
      outageRef.current = false;
      setOutage(false);
      setStreamNonce((n) => n + 1);
    }
  }, [outage, recoveryQ.data]);

  // Saved progress (audio choice + last position) for this user/file.
  // No `staleTime: Infinity` — we want a fresh read every time the user
  // mounts the page (otherwise navigating away and back would replay the
  // cached null/old value as if the user hadn't watched anything).
  const progressQ = useQuery({
    queryKey: ["progress", infohash, fileIdx],
    queryFn: () => progressApi.get(infohash, fileIdx),
    enabled: !!infohash,
  });

  // Per-user preferred audio + subtitle LANGUAGE (cross-episode / cross-device).
  // Applied only when this file has no saved per-file track (see IrisPlayer):
  // per-file index wins, else this language pref, else the file default.
  const playbackPrefsQ = useQuery({
    queryKey: ["playback-prefs"],
    queryFn: meApi.playbackPreferences,
    staleTime: 5 * 60 * 1000,
  });
  // Latest known language prefs, mirrored in a ref so the track-change
  // handlers send the full current state without re-rendering the player.
  const playbackPrefsRef = useRef<PlaybackPrefs>({
    audio_language: null,
    subtitle_language: null,
  });
  useEffect(() => {
    if (playbackPrefsQ.data) playbackPrefsRef.current = playbackPrefsQ.data;
  }, [playbackPrefsQ.data]);

  // All progress for this torrent (powers the "watched %" per episode in the
  // other-files panel).
  const torrentProgressQ = useQuery({
    queryKey: ["torrent-progress", infohash],
    queryFn: () => progressApi.forTorrent(infohash),
    enabled: !!infohash,
    refetchInterval: 10_000,
  });
  const progressByFileIdx = useMemo(() => {
    const map = new Map<number, FileProgressEntry>();
    for (const p of torrentProgressQ.data ?? []) {
      map.set(p.file_idx, p);
    }
    return map;
  }, [torrentProgressQ.data]);

  // Parent collection — the source of sibling episodes. A season pack
  // carries every episode as a file of THIS torrent (covered by
  // `videoFiles`), but a season grabbed as separate per-episode torrents
  // spreads them across many torrents; those siblings live on the
  // collection's merged episode list, not on `data.files`. Pulling the
  // collection lets the side panel offer the rest of the season either way.
  const collectionId = data?.collection_id ?? null;
  const collectionQ = useQuery({
    queryKey: ["collection", collectionId],
    queryFn: () => library.collection(collectionId!),
    enabled: !!collectionId && data?.kind === "tv",
  });
  const collectionEpisodes = collectionQ.data?.episodes;
  // Episodes Iris has discovered on a tracker but not grabbed yet — same
  // field CollectionPage renders as "available" chips, where every
  // language variant gets its own chip so the user can explicitly pick one.
  // This side panel is a compact "what's next" list, not a language picker,
  // so it dedupes to ONE row per (season, episode) below — see `discovered`.
  const availableEpisodes = collectionQ.data?.available_episodes ?? NO_AVAILABLE_EPISODES;
  const isTvCollection = !!collectionId && data?.kind === "tv";

  // The episode currently playing, looked up in the collection's merged
  // list — drives both the season scope and the language preference below.
  const currentEpisode = useMemo(
    () => (collectionEpisodes ?? []).find((e) => e.infohash === infohash && e.file_idx === fileIdx),
    [collectionEpisodes, infohash, fileIdx],
  );
  // `null` when the current file isn't in the collection's episode list yet
  // (e.g. a season pack not individually indexed) — the side panel then
  // shows every season rather than nothing.
  const currentSeason = currentEpisode?.season ?? null;

  // The language to prefer when an episode has several offers (Multi / FR /
  // EN, …): the language of the file currently playing, falling back to
  // whichever language is most common among already-downloaded episodes
  // (the series' "dominant owned language" — same concept the prepare-next
  // flow already uses server-side).
  const currentLanguage = useMemo(() => {
    if (currentEpisode?.language && currentEpisode.language !== "unknown") {
      return currentEpisode.language;
    }
    const counts = new Map<string, number>();
    for (const e of collectionEpisodes ?? []) {
      if (e.language && e.language !== "unknown") {
        counts.set(e.language, (counts.get(e.language) ?? 0) + 1);
      }
    }
    let best: string | null = null;
    let bestCount = 0;
    for (const [lang, count] of counts) {
      if (count > bestCount) {
        best = lang;
        bestCount = count;
      }
    }
    return best;
  }, [collectionEpisodes, currentEpisode]);

  // Side-panel rows: the collection's episodes (downloaded + discovered)
  // when this is a TV collection (so separate-episode torrents list the
  // whole season), else the current torrent's video files (season pack /
  // movie extras / orphan). Episode rows link to their own
  // `(infohash, fileIdx)`; discovered-but-ungrabbed rows carry a `grab`
  // payload instead and trigger `grabMutation` on click. Scoped to the
  // CURRENT season only — a multi-season collection otherwise dumps every
  // season's episodes into one flat list.
  const sideRows = useMemo<SideRow[]>(() => {
    if (isTvCollection && ((collectionEpisodes?.length ?? 0) > 0 || availableEpisodes.length > 0)) {
      const seasonEpisodes =
        currentSeason != null
          ? (collectionEpisodes ?? []).filter((e) => e.season === currentSeason)
          : (collectionEpisodes ?? []);
      const seasonAvailable =
        currentSeason != null
          ? availableEpisodes.filter((a) => a.season === currentSeason)
          : availableEpisodes;
      // One row per (season, episode), same dedup as `discovered` below:
      // a household can have the SAME episode downloaded more than once
      // (a French release grabbed today, an old English one from before) —
      // without this, the panel showed every duplicate as its own row,
      // languages mixed together. Preference order: whichever file is
      // actually playing right now (so the active row never disappears),
      // else `currentLanguage`, else Multi, else whatever's first.
      const downloadedBySeasonEpisode = new Map<string, typeof seasonEpisodes>();
      for (const e of seasonEpisodes) {
        const key = `${e.season}:${e.episode}`;
        const list = downloadedBySeasonEpisode.get(key);
        if (list) list.push(e);
        else downloadedBySeasonEpisode.set(key, [e]);
      }
      const downloaded = Array.from(downloadedBySeasonEpisode.values()).map((variants) => {
        const e =
          variants.find((v) => v.infohash === infohash && v.file_idx === fileIdx) ||
          (currentLanguage && variants.find((v) => v.language === currentLanguage)) ||
          variants.find((v) => v.language === "multi") ||
          variants[0]!;
        const prog = e.infohash === infohash ? progressByFileIdx.get(e.file_idx) : undefined;
        const lang = e.language && e.language !== "unknown" ? e.language : "";
        return {
          key: `dl:${e.infohash}:${e.file_idx}`,
          infohash: e.infohash,
          fileIdx: e.file_idx,
          primary: `S${String(e.season).padStart(2, "0")}E${String(e.episode).padStart(2, "0")}`,
          secondary: lang,
          mono: false,
          watched: e.watched || !!prog?.completed,
          watchedPct: watchedPctOf(prog),
          active: e.infohash === infohash && e.file_idx === fileIdx,
        } satisfies SideRow;
      });
      // One row per (season, episode): pick the variant matching
      // `currentLanguage`, else a Multi offer (it satisfies any language
      // preference), else whatever's first — never show the same episode
      // 2-3 times over because it happens to be offered in Multi/FR/EN.
      // Also drop any (season, episode) already covered by a `downloaded`
      // row above — the collection API deliberately keeps an alternate-
      // language offer "available" even once one language is owned (so
      // `CollectionPage`'s full chip picker can still surface it), but
      // this compact panel promises one row per episode: an episode
      // already on disk (e.g. auto-grabbed by prepare-next in the
      // series' dominant language) must never also show a redundant
      // "grab the other language" row next to it.
      const seasonAvailableUnowned = seasonAvailable.filter(
        (a) => !downloadedBySeasonEpisode.has(`${a.season}:${a.episode}`),
      );
      const bySeasonEpisode = new Map<string, AvailableEpisodeEntry[]>();
      for (const a of seasonAvailableUnowned) {
        const key = `${a.season}:${a.episode}`;
        const list = bySeasonEpisode.get(key);
        if (list) list.push(a);
        else bySeasonEpisode.set(key, [a]);
      }
      const discovered = Array.from(bySeasonEpisode.values()).map((variants) => {
        const a =
          (currentLanguage && variants.find((v) => v.language === currentLanguage)) ||
          variants.find((v) => v.language === "multi") ||
          variants[0]!;
        const lang = a.language && a.language !== "unknown" ? a.language : "";
        return {
          key: `av:${a.season}:${a.episode}`,
          infohash: "",
          fileIdx: -1,
          primary: `S${String(a.season).padStart(2, "0")}E${String(a.episode).padStart(2, "0")}`,
          secondary: lang,
          mono: false,
          watched: false,
          watchedPct: null,
          active: false,
          grab: { season: a.season, episode: a.episode, language: a.language ?? null },
        } satisfies SideRow;
      });
      return [...downloaded, ...discovered].sort((a, b) => a.primary.localeCompare(b.primary));
    }
    return videoFiles.map((f) => {
      const prog = progressByFileIdx.get(f.index);
      return {
        key: `f:${f.index}`,
        infohash: infohash ?? "",
        fileIdx: f.index,
        primary: f.path.split("/").pop() ?? f.path,
        secondary: formatSize(f.size_bytes),
        mono: true,
        watched: !!prog?.completed,
        watchedPct: watchedPctOf(prog),
        active: f.index === fileIdx,
      };
    });
  }, [
    isTvCollection,
    collectionEpisodes,
    availableEpisodes,
    currentSeason,
    currentLanguage,
    videoFiles,
    progressByFileIdx,
    infohash,
    fileIdx,
  ]);

  // Grabs a discovered-but-ungrabbed episode then jumps straight to it —
  // single action, mirrors the Play button's own navigate-on-click.
  const grabMutation = useMutation({
    mutationFn: ({ season, episode, language }: NonNullable<SideRow["grab"]>) =>
      library.grabCollectionEpisode(collectionId!, season, episode, language),
    onSuccess: (res) => {
      void qc.invalidateQueries({ queryKey: ["collection", collectionId] });
      navigate({
        to: "/watch/$infohash/$idx",
        params: { infohash: res.infohash, idx: String(res.file_idx) },
      });
    },
  });

  const sidePanelTitle =
    isTvCollection && ((collectionEpisodes?.length ?? 0) > 0 || availableEpisodes.length > 0)
      ? "Episodes"
      : "Other files";

  // First-load resume position. Applied once via `onCanPlay` (see below) —
  // we want a single deterministic seek before playback starts, not a
  // controlled `currentTime` prop that fights every user scrub.
  const startPosition = useMemo(() => {
    if (progressQ.isPending || !progressQ.data) return 0;
    if (progressQ.data.completed) return 0;
    return progressQ.data.position_seconds > 5 ? progressQ.data.position_seconds : 0;
  }, [progressQ.data, progressQ.isPending]);

  // Reset all per-file state when we navigate.
  useEffect(() => {
    setPlayerError(null);
    lastTimeRef.current = 0;
    lastSavedTimeRef.current = 0;
    lastDurationRef.current = null;
    progressLoadedRef.current = false;
    seekPendingRef.current = false;
    subtitleTrackRef.current = null;
    audioTrackRef.current = null;
    nextEpDismissedRef.current = false;
    nextEpPromptedRef.current = false;
    setNextEpModalOpen(false);
    autoAdvanceOffRef.current = false;
    setAutoAdvanceIn(null);
    outageRef.current = false;
    setOutage(false);
    setStreamNonce(0);
  }, [fileIdx, infohash]);

  // Episode context drives the "Watch next?" modal at episode
  // end. Returns nulls for non-TV files so the prompt simply never
  // fires — no need to gate the query.
  const episodeContextQ = useQuery({
    queryKey: ["episode-context", infohash, fileIdx],
    queryFn: () => follows.episodeContext(infohash, fileIdx),
    enabled: !!infohash,
    staleTime: 5 * 60_000,
  });
  const nextEp = episodeContextQ.data?.next;
  const canPromptNext =
    episodeContextQ.data?.followed === true &&
    nextEp?.status === "available" &&
    !nextEpDismissedRef.current;

  // The next episode is already on disk, so the player can jump straight to
  // it. `status === "available"` is the other case entirely: nothing to play
  // yet, which is what the grab dialog below is for.
  const autoAdvanceTarget =
    nextEp?.status === "downloaded" && nextEp.infohash && nextEp.file_idx != null
      ? {
          infohash: nextEp.infohash,
          fileIdx: nextEp.file_idx,
          season: nextEp.season,
          episode: nextEp.episode,
        }
      : null;

  const autoAdvanceRef = useRef(false);
  const goToNextEpisodeRef = useRef<() => void>(() => {});

  const goToNextEpisode = () => {
    if (!autoAdvanceTarget) return;
    autoAdvanceOffRef.current = true;
    setAutoAdvanceIn(null);
    navigate({
      to: "/watch/$infohash/$idx",
      params: {
        infohash: autoAdvanceTarget.infohash,
        idx: String(autoAdvanceTarget.fileIdx),
      },
    });
  };

  autoAdvanceRef.current = autoAdvanceTarget != null && !autoAdvanceOffRef.current;
  goToNextEpisodeRef.current = goToNextEpisode;

  function maybePromptNext() {
    if (!canPromptNext || nextEpPromptedRef.current) return;
    nextEpPromptedRef.current = true;
    setNextEpModalOpen(true);
  }

  // Unified player callbacks — defined once, fed into <IrisPlayer> which
  // forwards them to whichever tier engine is active.
  const onTimeUpdate = useCallback(
    (t: number) => {
      if (t > 0) lastTimeRef.current = t;
      if (infohash && t > 5 && t - lastSavedTimeRef.current > 7) {
        lastSavedTimeRef.current = t;
        const dur = lastDurationRef.current ?? null;
        const completed = isWatched(t, dur);
        const seek = seekPendingRef.current;
        seekPendingRef.current = false;
        void progressApi.put(infohash, fileIdx, {
          position_seconds: t,
          duration_seconds: dur,
          audio_track_idx: audioTrackRef.current,
          subtitle_track_idx: subtitleTrackRef.current,
          completed,
          playing: true,
          seek,
        });
      }
      const totalDur = lastDurationRef.current;
      // Auto-advance card. `timeupdate` fires several times a second, so
      // clamp to whole seconds and let React bail out when the displayed
      // number has not moved: ~20 renders for the whole countdown, and none
      // outside the window. Seeking backwards widens `remaining` and the
      // card withdraws on its own.
      if (autoAdvanceRef.current && totalDur != null && totalDur > 0) {
        const remaining = totalDur - t;
        const secs =
          remaining > 0 && remaining <= AUTO_ADVANCE_LEAD_S ? Math.ceil(remaining) : null;
        setAutoAdvanceIn((prev) => (prev === secs ? prev : secs));
      }
      // Next-episode prompt at >= 95 % of duration. Belt-and-suspenders
      // with onEnded — short episodes can skip the threshold sample.
      if (
        totalDur != null &&
        totalDur > 0 &&
        t / totalDur >= 0.95 &&
        canPromptNext &&
        !nextEpPromptedRef.current
      ) {
        maybePromptNext();
      }
    },
    // maybePromptNext / canPromptNext are recomputed each render but
    // capture stable refs; the eslint exhaustive-deps rule misfires
    // here, ignore by listing only the load-bearing identities.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
    [infohash, fileIdx, canPromptNext],
  );
  const onDurationChange = useCallback((d: number) => {
    if (d > 0) lastDurationRef.current = d;
  }, []);
  const onPause = useCallback(
    (t: number) => {
      if (!infohash || t <= 0) return;
      lastSavedTimeRef.current = t;
      const seek = seekPendingRef.current;
      seekPendingRef.current = false;
      void progressApi.put(infohash, fileIdx, {
        position_seconds: t,
        duration_seconds: lastDurationRef.current ?? null,
        audio_track_idx: audioTrackRef.current,
        subtitle_track_idx: subtitleTrackRef.current,
        completed: false,
        playing: false,
        seek,
      });
    },
    [infohash, fileIdx],
  );
  const onEndedCb = useCallback(() => {
    if (infohash) {
      void progressApi.put(infohash, fileIdx, {
        position_seconds: lastDurationRef.current ?? lastTimeRef.current,
        duration_seconds: lastDurationRef.current,
        audio_track_idx: audioTrackRef.current,
        subtitle_track_idx: subtitleTrackRef.current,
        completed: true,
      });
    }
    if (autoAdvanceRef.current) {
      goToNextEpisodeRef.current();
      return;
    }
    maybePromptNext();
    // Same reason as `onTimeUpdateCb` above: `maybePromptNext` is rebuilt
    // every render but only reads refs, so listing it would re-create this
    // callback on every render for nothing.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [infohash, fileIdx]);

  // Capture saved audio + subtitle picks so the next progress save
  // round-trips them (without this, the first auto-save after a fresh
  // mount would clobber the server-side value with `null` because
  // the user hasn't touched the menus yet).
  useEffect(() => {
    if (progressLoadedRef.current) return;
    if (progressQ.isPending) return;
    const p = progressQ.data;
    if (p?.subtitle_track_idx != null) {
      subtitleTrackRef.current = p.subtitle_track_idx;
    }
    if (p?.audio_track_idx != null) {
      audioTrackRef.current = p.audio_track_idx;
    }
    progressLoadedRef.current = true;
  }, [progressQ.data, progressQ.isPending]);

  // Best-effort save of the latest known position when the user navigates
  // away (other file, route change, tab close). Uses sendBeacon on unload so
  // the request actually leaves the browser.
  useEffect(() => {
    if (!infohash) return;
    const flush = () => {
      const t = lastTimeRef.current;
      if (t <= 0 || t === lastSavedTimeRef.current) return;
      lastSavedTimeRef.current = t;
      const dur = lastDurationRef.current ?? null;
      const completed = isWatched(t, dur);
      const body = JSON.stringify({
        position_seconds: t,
        duration_seconds: dur,
        audio_track_idx: audioTrackRef.current,
        subtitle_track_idx: subtitleTrackRef.current,
        completed,
        seek: seekPendingRef.current,
      });
      const url = `/api/torrents/${infohash}/files/${fileIdx}/progress`;
      // sendBeacon on unload; fall back to fire-and-forget fetch in normal flow.
      if (typeof navigator !== "undefined" && navigator.sendBeacon) {
        const blob = new Blob([body], { type: "application/json" });
        navigator.sendBeacon(url, blob);
      } else {
        void fetch(url, {
          method: "PUT",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body,
          keepalive: true,
        });
      }
    };
    const onUnload = () => flush();
    window.addEventListener("pagehide", onUnload);
    window.addEventListener("beforeunload", onUnload);
    return () => {
      window.removeEventListener("pagehide", onUnload);
      window.removeEventListener("beforeunload", onUnload);
      flush();
    };
  }, [infohash, fileIdx]);

  if (!infohash)
    return (
      <Container>
        <p className="py-10">Missing infohash.</p>
      </Container>
    );
  if (torrentQ.isLoading)
    return (
      <Container>
        <p className="py-10 text-muted-foreground">Loading…</p>
      </Container>
    );
  if (torrentQ.error) {
    // 404 = the engine no longer serves this infohash (GC-reclaimed, or a
    // stale link). Anything else is transient server trouble — show the
    // message, the 3s refetch keeps retrying behind it.
    const gone = torrentQ.error instanceof ApiError && torrentQ.error.status === 404;
    return gone ? (
      <NotFoundState
        eyebrow="Not available"
        title="This file is no longer on disk"
        description={
          regrab.isError
            ? "Automatic re-grab isn't possible for this release — find it again from the library or search."
            : "It was probably reclaimed to free up space. Grab it again and playback picks up right where it left off."
        }
        actions={
          <>
            {!regrab.isError && (
              <Button
                onClick={() => regrab.mutate()}
                disabled={regrab.isPending || regrab.isSuccess}
              >
                {regrab.isPending || regrab.isSuccess ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Download className="size-4" />
                )}
                {regrab.isPending ? "Grabbing…" : regrab.isSuccess ? "Starting…" : "Grab it again"}
              </Button>
            )}
            <Button variant="secondary" render={<Link to="/library" />}>
              Open library
            </Button>
          </>
        }
      />
    ) : (
      <NotFoundState
        eyebrow="Playback"
        title="Couldn't load this stream"
        description={torrentQ.error instanceof Error ? torrentQ.error.message : undefined}
      />
    );
  }
  if (!data)
    return (
      <NotFoundState
        eyebrow="Not available"
        title="This file is no longer on disk"
        description="It may have been reclaimed to free up space. You can grab it again from the library."
      />
    );

  const fileName = file?.path.split("/").pop() ?? data.name ?? "Iris";
  const downBps = data.download_speed_bps;
  const upBps = data.upload_speed_bps;
  const pct = Math.min(100, Math.max(0, data.progress_pct));
  // Tier A: direct <video src> over /stream — bypasses ffmpeg+shaka.
  // Tier B: Mediabunny demux + remux to fMP4 → MSE.
  // Tier C/D: WebCodecs decode → Canvas2D (rendered via <TierCPlayer>).
  // Tier F: legacy server-side HLS remux, the final fallback.
  const playSrcBase =
    tier === "F" ? torrents.playUrl(infohash, fileIdx) : rawStreamUrl(infohash, fileIdx);
  // `streamNonce` bumps after a backend outage to force IrisPlayer to
  // re-mount the engine on the SAME tier (a `src` change is what
  // re-triggers its mount effect). The backend ignores the extra param;
  // hls.js resolves variant/segment URLs against the path, not the query.
  const playSrc =
    streamNonce > 0
      ? `${playSrcBase}${playSrcBase.includes("?") ? "&" : "?"}_r=${streamNonce}`
      : playSrcBase;
  const playSrcType = tier === "F" ? "application/vnd.apple.mpegurl" : "video/mp4";
  // Only Tier F polls /play/status (it's the only path that gates on a
  // server-side remux). Everything else is ready once the manifest is.
  const sourceReady = tier === "F" ? playReady && probe != null : manifest != null;
  void hlsUrl; // kept exported for parity; no direct use here yet

  return (
    <Container wide={theater}>
      <div className="grid gap-6 pt-2">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h1 className="heading-2 [overflow-wrap:anywhere]" title={fileName}>
              {fileName}
            </h1>
            <p className="mt-0.5 text-sm text-muted-foreground [overflow-wrap:anywhere]">
              {data.name}
              {data.source_provider && (
                <span className="ml-2 font-mono text-xs text-fg-dim">
                  via {data.source_provider}
                </span>
              )}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant={theater ? "secondary" : "ghost"}
              size="sm"
              onClick={toggleTheater}
              aria-pressed={theater}
              title="Theater mode (t)"
              // Below lg the layout is single-column already — nothing
              // to enlarge, so the toggle only shows where it acts.
              className="hidden lg:inline-flex"
            >
              <RectangleHorizontal className="size-4" />
              Theater
            </Button>
            <Button
              variant="outline"
              size="sm"
              // Base UI merges the Button's children into the rendered <a>, so the
              // anchor does get content — the rule only sees the empty literal.
              // oxlint-disable-next-line jsx-a11y/anchor-has-content, jsx-a11y/control-has-associated-label
              render={<a href={torrents.downloadUrl(infohash, fileIdx)} download={fileName} />}
            >
              <Download className="size-4" />
              Download
            </Button>
            <Button variant="ghost" size="sm" render={<Link to="/library" />}>
              <LibraryIcon className="size-4" />
              Library
            </Button>
          </div>
        </div>

        {playerError && (
          <div className="rounded-xl border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
            Player error: {playerError}
          </div>
        )}

        {outage && (
          <div className="flex items-center gap-2 rounded-xl border border-warn/50 bg-warn/10 p-3 text-sm text-warn">
            <Loader2 className="size-4 animate-spin" />
            Server unavailable, reconnecting…
          </div>
        )}

        <div
          className={cn(
            "grid gap-6",
            sideRows.length > 1 && !theater && "lg:grid-cols-[minmax(0,1fr)_340px] lg:items-start",
          )}
        >
          <div className="grid min-w-0 gap-6">
            <div
              className={cn(
                "relative aspect-video w-full overflow-hidden rounded-xl border border-border bg-black shadow-2xl",
                // Theater: full-width strip, height capped to the viewport
                // (minus header + title row) — the video letterboxes inside
                // via its own `object-contain`, YouTube-style.
                theater && "max-h-[calc(100svh-var(--header-h)-8rem)]",
              )}
            >
              {playSrc && !progressQ.isPending && sourceReady && manifest ? (
                <IrisPlayer
                  // Per-file identity: navigating to another episode must
                  // rebuild the player from scratch (fresh `currentTimeRef`,
                  // audio/subtitle state) instead of reusing per-file state
                  // initialized for the previous file. Outage-recovery and
                  // demote remounts keep the same key — only `src` changes —
                  // so the live playhead survives those.
                  key={`${infohash}:${fileIdx}`}
                  tier={tier}
                  src={playSrc}
                  srcType={playSrcType}
                  title={fileName}
                  manifest={manifest}
                  startPosition={startPosition}
                  initialAudioIndex={progressQ.data?.audio_track_idx ?? undefined}
                  initialSubtitleStreamIdx={progressQ.data?.subtitle_track_idx ?? undefined}
                  preferredAudioLang={playbackPrefsQ.data?.audio_language ?? null}
                  preferredSubtitleLang={playbackPrefsQ.data?.subtitle_language ?? null}
                  initialVolume={readStoredVolume()}
                  onVolumeChange={(v) => writeStoredVolume(v)}
                  subtitleVersion={subtitleVersion}
                  onAudioTrackChange={(idx) => {
                    audioTrackRef.current = idx;
                    // Remember the chosen audio LANGUAGE per-user so it carries
                    // to the next episode / device (best-effort: a track may
                    // have no language tag → leave the pref unchanged).
                    const lang = manifest.audio[idx]?.lang;
                    if (lang) {
                      const next = { ...playbackPrefsRef.current, audio_language: lang };
                      playbackPrefsRef.current = next;
                      void meApi.savePlaybackPreferences(next);
                    }
                  }}
                  onActiveSubtitleChange={(streamIdx) => {
                    subtitleTrackRef.current = streamIdx;
                    // Persist the subtitle LANGUAGE preference: "off" when the
                    // user disabled subs, else the picked track's language.
                    const lang =
                      streamIdx == null
                        ? "off"
                        : (manifest.subtitles.find((s) => s.stream_idx === streamIdx)?.lang ??
                          null);
                    if (lang) {
                      const next = { ...playbackPrefsRef.current, subtitle_language: lang };
                      playbackPrefsRef.current = next;
                      void meApi.savePlaybackPreferences(next);
                    }
                  }}
                  onTimeUpdate={onTimeUpdate}
                  onDurationChange={onDurationChange}
                  onSeeking={(t) => {
                    seekPendingRef.current = true;
                    postSeekHint(manifest, t);
                  }}
                  onPause={onPause}
                  onEnded={onEndedCb}
                  onError={(msg) => {
                    // Routed through `handleEngineError`: a transient backend
                    // outage (502/503/504 during a deploy) holds the tier and
                    // reconnects instead of demoting. Genuine errors with the
                    // backend up then either surface the banner (A/F) or demote
                    // to the server-side HLS fallback (B/C/D/E).
                    void handleEngineError(tier, msg);
                  }}
                />
              ) : (
                <PlayerLoadingStatus
                  torrent={data}
                  probeFetching={probeQ.isFetching}
                  probeError={probeQ.error}
                  progressPending={progressQ.isPending}
                  playStatus={playStatusQ.data ?? null}
                  playError={playStatusQ.error}
                />
              )}

              {/* Auto-advance. Sibling of the player, not a child: a click
                  here must not reach the video surface, which toggles
                  playback. The countdown is the file's own remaining time, so
                  nothing ticks it. */}
              {autoAdvanceIn != null && autoAdvanceTarget && (
                <div className="absolute right-4 bottom-20 z-10 max-w-[calc(100%-2rem)] rounded-lg border border-border bg-background/95 p-3 shadow-2xl backdrop-blur">
                  <p className="text-xs text-muted-foreground">Up next</p>
                  <p className="mt-0.5 text-sm font-medium">
                    S{autoAdvanceTarget.season.toString().padStart(2, "0")}E
                    {autoAdvanceTarget.episode.toString().padStart(2, "0")} in {autoAdvanceIn}s
                  </p>
                  <div className="mt-2 flex justify-end gap-2">
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => {
                        autoAdvanceOffRef.current = true;
                        setAutoAdvanceIn(null);
                      }}
                    >
                      Cancel
                    </Button>
                    <Button size="sm" onClick={goToNextEpisode}>
                      <Play className="size-3.5" />
                      Play now
                    </Button>
                  </div>
                </div>
              )}
            </div>

            {probe && probe.subtitle.length > 0 && (
              <div className="glass grid gap-3 rounded-xl p-4 text-sm">
                <div className="grid gap-1 text-xs">
                  <span className="eyebrow">Subtitles ({probe.subtitle.length})</span>
                  <ul className="grid gap-0.5 text-muted-foreground">
                    {probe.subtitle.map((s) => (
                      <li key={s.index}>
                        <span className="text-foreground">
                          {s.title ?? s.language?.toUpperCase() ?? `Sub ${s.index + 1}`}
                        </span>
                        <span className="ml-2 text-[11px]">
                          {s.codec}
                          {s.forced ? " · forced" : ""}
                          {s.default ? " · default" : ""}
                        </span>
                      </li>
                    ))}
                  </ul>
                  <span className="text-[11px]">
                    Switch subtitles and audio tracks from the player menu.
                  </span>
                </div>
              </div>
            )}

            <section className="glass grid gap-2 rounded-xl p-4">
              <div className="flex items-center justify-between text-sm">
                <StateBadge state={data.state} />
                <span>
                  {formatSize(data.progress_bytes)} / {formatSize(data.total_size_bytes)} ·{" "}
                  {pct.toFixed(1)}%
                </span>
              </div>
              <Progress value={pct} />
              <div className="flex flex-wrap items-center gap-x-6 gap-y-1 text-xs text-muted-foreground">
                <span>↓ {formatSize(downBps)}/s</span>
                <span>↑ {formatSize(upBps)}/s</span>
                <span>{data.peers} peers</span>
                {probe?.video[0] && (
                  <span>
                    {probe.video[0].codec.toUpperCase()}{" "}
                    {probe.video[0].width &&
                      probe.video[0].height &&
                      `${probe.video[0].width}×${probe.video[0].height}`}
                  </span>
                )}
                {probe?.duration_seconds && <span>{formatDuration(probe.duration_seconds)}</span>}
                {data.error && <span className="text-destructive">error: {data.error}</span>}
              </div>
            </section>
          </div>

          {sideRows.length > 1 && (
            <aside
              className={cn(
                "glass grid h-fit gap-3 self-start rounded-xl p-4",
                // Side-column mode pins the panel and scrolls it
                // internally. In theater it flows BELOW the full-width
                // player (same stacking as the responsive layout), where
                // pinning/inner-scroll would fight the page scroll.
                !theater && "lg:sticky lg:top-20 lg:max-h-[calc(100svh-5.5rem)] lg:overflow-auto",
              )}
            >
              <span className="eyebrow">
                {sidePanelTitle} ({sideRows.length})
              </span>
              <ul className="grid gap-1">
                {sideRows.map((row) => (
                  <li
                    key={row.key}
                    className={cn(
                      "grid gap-2 rounded-lg px-2.5 py-2 text-sm",
                      row.active ? "bg-accent text-accent-foreground" : "hover:bg-hover",
                    )}
                  >
                    <div className="min-w-0">
                      <div
                        className={cn("truncate", row.mono ? "font-mono text-xs" : "font-medium")}
                        title={row.primary}
                      >
                        {row.primary}
                      </div>
                      <div className="mt-0.5 flex items-center gap-2 text-[11px] text-muted-foreground">
                        {row.secondary && <span>{row.secondary}</span>}
                        {row.watched && (
                          <span className="inline-flex items-center gap-0.5 text-success">
                            <CheckCircle2 className="size-3" />
                            watched
                          </span>
                        )}
                        {!row.watched && row.watchedPct != null && row.watchedPct > 0 && (
                          <span className="text-success">{row.watchedPct.toFixed(0)}%</span>
                        )}
                        {row.active && <span className="text-foreground/80">· now playing</span>}
                      </div>
                      {!row.watched && row.watchedPct != null && row.watchedPct > 0 && (
                        <Progress className="mt-1 h-0.5" value={row.watchedPct} />
                      )}
                    </div>
                    <div className="flex items-center gap-1.5">
                      {row.grab ? (
                        <Button
                          size="sm"
                          className="flex-1"
                          disabled={
                            grabMutation.isPending &&
                            grabMutation.variables?.season === row.grab.season &&
                            grabMutation.variables?.episode === row.grab.episode
                          }
                          onClick={() => grabMutation.mutate(row.grab!)}
                        >
                          {grabMutation.isPending &&
                          grabMutation.variables?.season === row.grab.season &&
                          grabMutation.variables?.episode === row.grab.episode ? (
                            <Loader2 className="size-3.5 animate-spin" />
                          ) : (
                            <Download className="size-3.5" />
                          )}
                          Grab & Play
                        </Button>
                      ) : (
                        <>
                          <Button
                            size="sm"
                            variant={row.active ? "secondary" : "default"}
                            disabled={row.active}
                            className="flex-1"
                            onClick={() =>
                              navigate({
                                to: "/watch/$infohash/$idx",
                                params: { infohash: row.infohash, idx: String(row.fileIdx) },
                              })
                            }
                          >
                            <Play className="size-3.5" />
                            {row.watchedPct != null && row.watchedPct > 0 && !row.watched
                              ? "Resume"
                              : "Play"}
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            render={
                              // Base UI merges the Button's children into the rendered <a>.
                              // oxlint-disable-next-line jsx-a11y/anchor-has-content, jsx-a11y/control-has-associated-label
                              <a
                                href={torrents.downloadUrl(row.infohash, row.fileIdx)}
                                download={row.primary}
                              />
                            }
                          >
                            <Download className="size-3.5" />
                            <span className="sr-only">Download</span>
                          </Button>
                        </>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            </aside>
          )}
        </div>

        {/* "Watch next?" — fired when the user is following the
          current series and the next episode is available but not yet
          grabbed. One-shot per file mount; "Later" silences it for
          the rest of the session. */}
        {nextEp && (
          <Dialog
            open={nextEpModalOpen}
            onOpenChange={(o) => {
              setNextEpModalOpen(o);
              if (!o) nextEpDismissedRef.current = true;
            }}
          >
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Next episode available</DialogTitle>
                <DialogDescription>
                  S{nextEp.season.toString().padStart(2, "0")}E
                  {nextEp.episode.toString().padStart(2, "0")} is ready to grab. Prepare it for the
                  next session?
                </DialogDescription>
              </DialogHeader>
              <div className="flex flex-wrap justify-end gap-2">
                <Button
                  variant="ghost"
                  onClick={() => {
                    nextEpDismissedRef.current = true;
                    setNextEpModalOpen(false);
                  }}
                >
                  Later
                </Button>
                <Button
                  disabled={nextEpGrabbing}
                  onClick={async () => {
                    if (!nextEp.follow_id) return;
                    setNextEpGrabbing(true);
                    try {
                      await follows.grabEpisode(nextEp.follow_id, nextEp.season, nextEp.episode);
                      setNextEpModalOpen(false);
                    } catch (e) {
                      console.error("[next-ep grab]", e);
                    } finally {
                      setNextEpGrabbing(false);
                    }
                  }}
                >
                  {nextEpGrabbing ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Download className="size-4" />
                  )}
                  Prepare
                </Button>
              </div>
            </DialogContent>
          </Dialog>
        )}
      </div>
    </Container>
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

function PlayerLoadingStatus({
  torrent,
  probeFetching,
  probeError,
  progressPending,
  playStatus,
  playError,
}: {
  torrent: TorrentView;
  probeFetching: boolean;
  probeError: unknown;
  progressPending: boolean;
  playStatus: PlayStatus | null;
  playError: unknown;
}) {
  const fileOnDisk =
    probeError == null ||
    !(probeError instanceof Error) ||
    !probeError.message.includes("not yet on disk");
  const downloadPct = Math.min(100, Math.max(0, torrent.progress_pct));

  type Step = { label: string; sub?: string; pct?: number };
  let step: Step;
  let isError = torrent.state === "error";
  if (torrent.state === "error") {
    step = {
      label: "Torrent error",
      sub: torrent.error ?? "Engine reported a fault. Try removing and re-adding.",
    };
  } else if (torrent.state === "initializing") {
    step = {
      label: "Initializing torrent…",
      sub: "Negotiating with peers and computing piece map.",
    };
  } else if (!fileOnDisk) {
    step = {
      label: `Buffering first bytes · ${downloadPct.toFixed(0)}%`,
      sub: `${formatSize(torrent.download_speed_bps)}/s · ${torrent.peers} peer${torrent.peers === 1 ? "" : "s"}`,
      pct: downloadPct,
    };
  } else if (probeFetching) {
    step = {
      label: "Reading media metadata…",
      sub: "ffprobe scanning streams (codec, audio, subtitles).",
    };
  } else if (probeError instanceof Error) {
    // Probe finished with a non-disk error (e.g., ffprobe crash, bad
    // permissions, corrupt file). Without this branch the user would
    // sit on the "Preparing playback…" fallback indefinitely while
    // the parent gate's `probe` check stays falsy.
    isError = true;
    step = {
      label: "Playback prep failed",
      sub: probeError.message,
    };
  } else if (progressPending) {
    step = {
      label: "Loading saved position…",
    };
  } else if (playStatus?.error) {
    isError = true;
    step = { label: "Playback prep failed", sub: playStatus.error };
  } else if (playError instanceof Error) {
    isError = true;
    step = { label: "Playback prep failed", sub: playError.message };
  } else if (!playStatus) {
    step = { label: "Starting playback prep…" };
  } else if (playStatus.reason === "downloading") {
    const pct =
      playStatus.progress != null
        ? Math.min(99, Math.max(0, playStatus.progress * 100))
        : downloadPct;
    step = {
      label: `Downloading · ${pct.toFixed(0)}%`,
      sub: `${formatSize(torrent.download_speed_bps)}/s · ${torrent.peers} peer${torrent.peers === 1 ? "" : "s"}`,
      pct,
    };
  } else if (playStatus.reason === "remuxing") {
    // Surface ffmpeg's encoded-so-far / total-duration so the bar
    // ticks instead of an indeterminate spinner. `progress` is null
    // until ffmpeg writes its first `out_time_us` block (~1s after
    // spawn) — fall back to the label-only state in that brief window.
    const pct =
      playStatus.progress != null
        ? Math.min(99, Math.max(0, playStatus.progress * 100))
        : undefined;
    step = {
      label:
        pct != null
          ? `Remuxing to fragmented MP4 · ${pct.toFixed(0)}%`
          : "Remuxing to fragmented MP4…",
      sub: "Producing the playable cache (HEVC/H.264 video copied as-is, audio re-encoded to AAC where needed).",
      pct,
    };
  } else {
    step = {
      label: "Preparing playback…",
      sub: "Almost there.",
    };
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      {isError ? (
        <span className="text-2xl text-destructive">!</span>
      ) : (
        <Loader2 className="size-8 animate-spin text-muted-foreground" />
      )}
      <div className="grid gap-1">
        <span className={`text-sm font-medium ${isError ? "text-destructive" : "text-foreground"}`}>
          {step.label}
        </span>
        {step.sub && <span className="text-xs text-muted-foreground">{step.sub}</span>}
      </div>
      {!isError && step.pct != null && (
        <div className="w-64">
          <Progress value={step.pct} className="h-1" />
        </div>
      )}
    </div>
  );
}

function formatDuration(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0) return `${h}h${m.toString().padStart(2, "0")}m`;
  return `${m}m${s.toString().padStart(2, "0")}`;
}
