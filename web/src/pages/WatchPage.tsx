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
      // Stop polling once the cache is ready OR a sticky failure is
      // surfaced — both are terminal until the user retries.
      if (!d || d.ready || d.error) return false as const;
      return 1000;
    },
    retry: 8,
    retryDelay: 2000,
  });
  const playReady = playStatusQ.data?.ready === true;

  // Gate probe on "download is finished" via playStatus, NOT on
  // `!!file`. Two reasons:
  //   1. `data.files` from /api/torrents can briefly be empty after a
  //      grab (librqbit metadata race) — gating on it left users stuck
  //      on "Preparing playback".
  //   2. ffprobe explodes on librqbit's pre-allocated zero-filled
  //      sparse files mid-download ("EBML header parsing failed").
  // playStatus reports `reason="downloading"` while bytes are still
  // arriving and flips to `"remuxing"` (or `ready: true`) the moment
  // the file is fully on disk. That's the precise signal we want — and
  // it lets us probe ONCE without the wasted retries that a fixed
  // timeout would impose on slow downloads.
  const downloadFinished =
    playStatusQ.data != null &&
    playStatusQ.data.reason !== "downloading" &&
    playStatusQ.data.error == null;
  const probeQ = useQuery({
    queryKey: ["probe", infohash, fileIdx],
    queryFn: () => torrents.probe(infohash!, fileIdx),
    enabled: !!infohash && downloadFinished,
    retry: (failureCount, err) => {
      // Tight retry budget — by the time we even fire this query the
      // download is already done per playStatus. The only "not yet on
      // disk" case left is the brief window between librqbit flushing
      // the last piece and the file being readable.
      const msg = err instanceof Error ? err.message : "";
      return msg.includes("not yet on disk") && failureCount < 5;
    },
    retryDelay: 2000,
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
    retry: (failureCount, err) =>
      err instanceof ManifestNotReadyError && failureCount < 30,
    retryDelay: 2000,
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
        const isHevc =
          primary != null && /hevc|hev1|hvc1|h265|x265/i.test(primary.codec);
        const within1080p = (primary?.height ?? 0) <= 1080 && (primary?.height ?? 0) > 0;
        const chromiumish =
          /Chrome|Edg/.test(navigator.userAgent) && !/Mobile/.test(navigator.userAgent);
        if (
          isHevc &&
          within1080p &&
          chromiumish &&
          !demotedRef.current.has("E")
        ) {
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
        void fetch(
          `/api/torrents/${manifest.infohash}/files/${manifest.file_idx}/playback-error`,
          {
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
          },
        ).catch(() => undefined);
      }
    },
    [manifest, nextDemotionTarget],
  );

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
    nextEpDismissedRef.current = false;
    nextEpPromptedRef.current = false;
    setNextEpModalOpen(false);
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
      subtitle_track_idx: subtitleTrackRef.current,
      completed: true,
    });
    maybePromptNext();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [infohash, fileIdx]);

  // Capture saved subtitle pick (the actual seek is applied in onCanPlay).
  useEffect(() => {
    if (progressLoadedRef.current) return;
    if (progressQ.isPending) return;
    const p = progressQ.data;
    if (p?.subtitle_track_idx != null) {
      subtitleTrackRef.current = p.subtitle_track_idx;
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
  const playSrc = tier === "F" ? torrents.playUrl(infohash, fileIdx) : rawStreamUrl(infohash, fileIdx);
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

      <div className="aspect-video w-full overflow-hidden rounded-lg border border-border bg-black">
        {playSrc && !progressQ.isPending && sourceReady && manifest ? (
          <IrisPlayer
            tier={tier}
            src={playSrc}
            srcType={playSrcType}
            title={fileName}
            manifest={manifest}
            startPosition={startPosition}
            onTimeUpdate={onTimeUpdate}
            onDurationChange={onDurationChange}
            onSeeking={(t) => postSeekHint(manifest, t)}
            onPause={onPause}
            onEnded={onEndedCb}
            onError={(msg) => {
              // Tier A → Vidstack: keep the legacy "Player error: …"
              //   banner so the user has feedback while we don't auto-
              //   demote (Vidstack errors are usually transient).
              // Tier B/C/D → demote to F. The legacy HLS pipeline always
              //   plays the file, at the cost of server-side ffmpeg.
              if (tier === "A" || tier === "F") setPlayerError(msg);
              else demoteTier(tier, msg);
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
                {nextEp.episode.toString().padStart(2, "0")} is ready to grab.
                Prepare it for the next session?
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
                    await follows.grabEpisode(
                      nextEp.follow_id,
                      nextEp.season,
                      nextEp.episode,
                    );
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
