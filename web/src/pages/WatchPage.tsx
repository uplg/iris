import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { CheckCircle2, Download, Library as LibraryIcon, Loader2, Play } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  follows,
  progress as progressApi,
  torrents,
  type FileEntry,
  type FileProgressEntry,
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

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

export function WatchPage() {
  const { infohash, idx } = useParams<{ infohash: string; idx: string }>();
  const fileIdx = Number(idx ?? 0);
  const navigate = useNavigate();

  const [playerError, setPlayerError] = useState<string | null>(null);
  const lastTimeRef = useRef(0);
  const lastSavedTimeRef = useRef(0);
  const lastDurationRef = useRef<number | null>(null);
  const progressLoadedRef = useRef(false);
  // "Watch next?" state — gated on a single-shot flip per
  // mount, plus a dismissal flag so user choosing "Later" doesn't
  // get re-prompted within the same session.
  const [nextEpModalOpen, setNextEpModalOpen] = useState(false);
  const [nextEpGrabbing, setNextEpGrabbing] = useState(false);
  const nextEpDismissedRef = useRef(false);
  const nextEpPromptedRef = useRef(false);
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
    queryFn: () => torrents.get(infohash!),
    enabled: !!infohash,
    refetchInterval: 3000,
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
    queryFn: () => torrents.playStatus(infohash!, fileIdx),
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
    queryFn: () => torrents.probe(infohash!, fileIdx),
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
    queryFn: () => fetchManifest(infohash!, fileIdx),
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
    queryFn: () => progressApi.get(infohash!, fileIdx),
    enabled: !!infohash,
  });

  // All progress for this torrent (powers the "watched %" per episode in the
  // other-files panel).
  const torrentProgressQ = useQuery({
    queryKey: ["torrent-progress", infohash],
    queryFn: () => progressApi.forTorrent(infohash!),
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
    subtitleTrackRef.current = null;
    audioTrackRef.current = null;
    nextEpDismissedRef.current = false;
    nextEpPromptedRef.current = false;
    setNextEpModalOpen(false);
    outageRef.current = false;
    setOutage(false);
    setStreamNonce(0);
  }, [fileIdx, infohash]);

  // Episode context drives the "Watch next?" modal at episode
  // end. Returns nulls for non-TV files so the prompt simply never
  // fires — no need to gate the query.
  const episodeContextQ = useQuery({
    queryKey: ["episode-context", infohash, fileIdx],
    queryFn: () => follows.episodeContext(infohash!, fileIdx),
    enabled: !!infohash,
    staleTime: 5 * 60_000,
  });
  const nextEp = episodeContextQ.data?.next;
  const canPromptNext =
    episodeContextQ.data?.followed === true &&
    nextEp?.status === "available" &&
    !nextEpDismissedRef.current;

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
        const completed = dur != null && t >= dur - 30;
        void progressApi.put(infohash, fileIdx, {
          position_seconds: t,
          duration_seconds: dur,
          audio_track_idx: audioTrackRef.current,
          subtitle_track_idx: subtitleTrackRef.current,
          completed,
        });
      }
      // Next-episode prompt at >= 95 % of duration. Belt-and-suspenders
      // with onEnded — short episodes can skip the threshold sample.
      const totalDur = lastDurationRef.current;
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [infohash, fileIdx, canPromptNext],
  );
  const onDurationChange = useCallback((d: number) => {
    if (d > 0) lastDurationRef.current = d;
  }, []);
  const onPause = useCallback(
    (t: number) => {
      if (!infohash || t <= 0) return;
      lastSavedTimeRef.current = t;
      void progressApi.put(infohash, fileIdx, {
        position_seconds: t,
        duration_seconds: lastDurationRef.current ?? null,
        audio_track_idx: audioTrackRef.current,
        subtitle_track_idx: subtitleTrackRef.current,
        completed: false,
      });
    },
    [infohash, fileIdx],
  );
  const onEndedCb = useCallback(() => {
    if (!infohash) return;
    void progressApi.put(infohash, fileIdx, {
      position_seconds: lastDurationRef.current ?? lastTimeRef.current,
      duration_seconds: lastDurationRef.current,
      audio_track_idx: audioTrackRef.current,
      subtitle_track_idx: subtitleTrackRef.current,
      completed: true,
    });
    maybePromptNext();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
      const completed = dur != null && t >= dur - 30;
      const body = JSON.stringify({
        position_seconds: t,
        duration_seconds: dur,
        audio_track_idx: audioTrackRef.current,
        subtitle_track_idx: subtitleTrackRef.current,
        completed,
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

  if (!infohash) return <p>Missing infohash.</p>;
  if (torrentQ.isLoading) return <p className="text-muted-foreground">Loading…</p>;
  if (torrentQ.error)
    return (
      <p className="text-destructive">
        {torrentQ.error instanceof Error ? torrentQ.error.message : "failed"}
      </p>
    );
  if (!data) return <p className="text-muted-foreground">Not found.</p>;

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
    <div className="grid gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="break-words text-xl font-semibold tracking-tight" title={fileName}>
            {fileName}
          </h1>
          <p className="mt-0.5 break-words text-sm text-muted-foreground">
            {data.name}
            {data.source_provider && (
              <span className="ml-2 text-xs text-muted-foreground/70">
                via {data.source_provider}
              </span>
            )}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button asChild variant="outline" size="sm">
            <a href={torrents.downloadUrl(infohash, fileIdx)} download={fileName}>
              <Download className="size-4" />
              Download
            </a>
          </Button>
          <Button asChild variant="ghost" size="sm">
            <Link to="/library">
              <LibraryIcon className="size-4" />
              Library
            </Link>
          </Button>
        </div>
      </div>

      {playerError && (
        <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          Player error: {playerError}
        </div>
      )}

      {outage && (
        <div className="flex items-center gap-2 rounded-md border border-amber-500/50 bg-amber-500/10 p-3 text-sm text-amber-200">
          <Loader2 className="size-4 animate-spin" />
          Server unavailable — reconnecting…
        </div>
      )}

      <div className="aspect-video w-full overflow-hidden rounded-lg border border-border bg-black">
        {playSrc && !progressQ.isPending && sourceReady && manifest ? (
          <IrisPlayer
            tier={tier}
            src={playSrc}
            srcType={playSrcType}
            title={fileName}
            manifest={manifest}
            startPosition={startPosition}
            initialAudioIndex={progressQ.data?.audio_track_idx ?? undefined}
            initialSubtitleStreamIdx={progressQ.data?.subtitle_track_idx ?? undefined}
            subtitleVersion={subtitleVersion}
            onAudioTrackChange={(idx) => {
              audioTrackRef.current = idx;
            }}
            onActiveSubtitleChange={(streamIdx) => {
              subtitleTrackRef.current = streamIdx;
            }}
            onTimeUpdate={onTimeUpdate}
            onDurationChange={onDurationChange}
            onSeeking={(t) => postSeekHint(manifest, t)}
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
      </div>

      {videoFiles.length > 1 && (
        <section className="grid gap-3 rounded-md border border-border bg-card/40 p-4">
          <span className="text-xs uppercase tracking-wide text-muted-foreground">
            Other files in this torrent ({videoFiles.length})
          </span>
          <ul className="grid gap-1">
            {videoFiles.map((f) => {
              const active = f.index === fileIdx;
              const fname = f.path.split("/").pop() ?? f.path;
              const prog = progressByFileIdx.get(f.index);
              const watchedPct =
                prog && prog.duration_seconds && prog.duration_seconds > 0
                  ? Math.min(100, (prog.position_seconds / prog.duration_seconds) * 100)
                  : null;
              return (
                <li
                  key={f.index}
                  className={`flex items-center justify-between gap-3 rounded px-2 py-1.5 text-sm ${
                    active ? "bg-accent text-accent-foreground" : "hover:bg-muted/40"
                  }`}
                >
                  <div className="min-w-0 flex-1">
                    <div className="break-all font-mono text-xs" title={f.path}>
                      {f.path}
                    </div>
                    <div className="mt-0.5 flex items-center gap-2 text-[11px] text-muted-foreground">
                      <span>{formatSize(f.size_bytes)}</span>
                      {prog?.completed && (
                        <span className="inline-flex items-center gap-0.5 text-emerald-300">
                          <CheckCircle2 className="size-3" />
                          watched
                        </span>
                      )}
                      {!prog?.completed && watchedPct != null && watchedPct > 0 && (
                        <span className="text-emerald-300">{watchedPct.toFixed(0)}%</span>
                      )}
                      {active && <span className="text-foreground/80">· now playing</span>}
                    </div>
                    {!prog?.completed && watchedPct != null && watchedPct > 0 && (
                      <Progress className="mt-1 h-0.5" value={watchedPct} />
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1.5">
                    <Button
                      size="sm"
                      variant={active ? "secondary" : "default"}
                      disabled={active}
                      onClick={() => navigate(`/watch/${infohash}/${f.index}`)}
                    >
                      <Play className="size-3.5" />
                      {watchedPct != null && watchedPct > 0 && !prog?.completed ? "Resume" : "Play"}
                    </Button>
                    <Button asChild size="sm" variant="outline">
                      <a href={torrents.downloadUrl(infohash, f.index)} download={fname}>
                        <Download className="size-3.5" />
                        <span className="sr-only">Download</span>
                      </a>
                    </Button>
                  </div>
                </li>
              );
            })}
          </ul>
        </section>
      )}

      {probe && probe.subtitle.length > 0 && (
        <div className="grid gap-3 rounded-md border border-border bg-card/40 p-4 text-sm">
          <div className="grid gap-1 text-xs">
            <span className="uppercase tracking-wide text-muted-foreground">
              Subtitles ({probe.subtitle.length})
            </span>
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

      <section className="grid gap-2 rounded-md border border-border bg-card/40 p-4">
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
