import "@vidstack/react/player/styles/default/theme.css";
import "@vidstack/react/player/styles/default/layouts/video.css";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import {
  CheckCircle2,
  Download,
  Library as LibraryIcon,
  Loader2,
  Play,
} from "lucide-react";
import {
  isHLSProvider,
  MediaPlayer,
  MediaProvider,
  Track,
  type MediaPlayerInstance,
} from "@vidstack/react";
import { defaultLayoutIcons, DefaultVideoLayout } from "@vidstack/react/player/layouts/default";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  progress as progressApi,
  torrents,
  type AudioStream,
  type FileEntry,
  type FileProgressEntry,
  type SubtitleStream,
  type TorrentView,
} from "@/lib/api";
import { formatSize } from "@/lib/format";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

export function WatchPage() {
  const { infohash, idx } = useParams<{ infohash: string; idx: string }>();
  const fileIdx = Number(idx ?? 0);
  const navigate = useNavigate();

  const [audioIdx, setAudioIdx] = useState<number | null>(null);
  const [playerError, setPlayerError] = useState<string | null>(null);
  // Use a ref (not state) for the player instance so callbacks always see
  // the latest value without depending on render-cycle ordering.
  const playerRef = useRef<MediaPlayerInstance | null>(null);
  const lastTimeRef = useRef(0);
  const lastSavedTimeRef = useRef(0);
  const lastDurationRef = useRef<number | null>(null);
  const [pendingSeek, setPendingSeek] = useState<number | null>(null);
  const progressLoadedRef = useRef(false);
  const subtitleTrackRef = useRef<number | null>(null);
  // Mirror to refs so the unmount cleanup save uses fresh values.
  const audioIdxRef = useRef<number | null>(null);
  audioIdxRef.current = audioIdx;

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

  const probeQ = useQuery({
    queryKey: ["probe", infohash, fileIdx],
    queryFn: () => torrents.probe(infohash!, fileIdx),
    enabled: !!infohash && !!file,
    retry: (failureCount, err) => {
      const msg = err instanceof Error ? err.message : "";
      return msg.includes("not yet on disk") && failureCount < 30;
    },
    retryDelay: 2000,
  });

  const probe = probeQ.data;
  const textSubs = useMemo<SubtitleStream[]>(
    () => probe?.subtitle.filter((s) => s.text_based) ?? [],
    [probe],
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

  // Poll the HLS prep status while we wait for ffmpeg to write ENDLIST.
  // This is the loading-state telemetry the user actually wants to see —
  // segments-produced counter ticking up so they know things are alive.
  // The HLS pipeline now produces a single master playlist with all audio
  // renditions baked in (via #EXT-X-MEDIA), so HLS prep no longer depends
  // on the audio pick — there's exactly one ffmpeg job per file.
  const hlsStatusQ = useQuery({
    queryKey: ["hls-status", infohash, fileIdx],
    queryFn: () => torrents.hlsStatus(infohash!, fileIdx),
    enabled: !!infohash,
    refetchInterval: (q) => {
      const data = q.state.data;
      // Stop polling once the playlist is finalized.
      return data?.endlist_present ? false : 1000;
    },
    retry: 8,
    retryDelay: 2000,
  });
  const masterReady = hlsStatusQ.data?.endlist_present === true;

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

  // Computed start position for hls.js. We hand this to the HLS provider so
  // it loads from the right offset natively — no mid-mount currentTime
  // assignment, which is what triggered AppleVTDecoder errors when the seek
  // collided with the first segment buffer.
  const startPosition = useMemo(() => {
    if (progressQ.isPending || !progressQ.data) return 0;
    if (progressQ.data.completed) return 0;
    return progressQ.data.position_seconds > 5 ? progressQ.data.position_seconds : 0;
  }, [progressQ.data, progressQ.isPending]);

  // Reset all per-file state when we navigate.
  useEffect(() => {
    setAudioIdx(null);
    setPendingSeek(null);
    setPlayerError(null);
    lastTimeRef.current = 0;
    lastSavedTimeRef.current = 0;
    lastDurationRef.current = null;
    progressLoadedRef.current = false;
    subtitleTrackRef.current = null;
  }, [fileIdx, infohash]);

  // Pick the audio track: saved one if any (and still present), else default.
  // Wait for BOTH probe and progress before deciding so we don't lock in the
  // default audio while saved progress is still loading.
  useEffect(() => {
    if (!probe || audioIdx != null) return;
    if (progressQ.isPending) return;
    if (probe.audio.length === 0) return;
    const saved = progressQ.data?.audio_track_idx;
    let chosen: number;
    if (saved != null && probe.audio.some((a) => a.index === saved)) {
      chosen = saved;
    } else {
      const def = probe.audio.find((a) => a.default) ?? probe.audio[0]!;
      chosen = def.index;
    }
    setAudioIdx(chosen);
  }, [probe, audioIdx, progressQ.data, progressQ.isPending]);

  // Capture saved subtitle pick (we don't need the seek anymore — startPosition
  // handles it).
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
        audio_track_idx: audioIdxRef.current ?? null,
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

  // pendingSeek is only used for *runtime* re-positioning (audio track switch).
  // First-load resume is handled by hls.js startPosition (set in the provider
  // config below), which avoids the mount→seek glitch that confuses Apple's
  // VideoToolbox decoder.
  useEffect(() => {
    if (pendingSeek == null) return;
    let cancelled = false;
    const apply = (): boolean => {
      const p = playerRef.current;
      if (!p) return false;
      const can = p.state?.canPlay ?? false;
      if (!can) return false;
      try {
        p.currentTime = pendingSeek;
      } catch (e) {
        tracingNoop(e);
      }
      setPendingSeek(null);
      return true;
    };
    if (apply()) return;
    const id = window.setInterval(() => {
      if (cancelled) return;
      if (apply()) {
        window.clearInterval(id);
      }
    }, 200);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pendingSeek]);

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
  const hlsSrc = torrents.hlsUrl(infohash, fileIdx);

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
        {hlsSrc && !progressQ.isPending && masterReady ? (
          <MediaPlayer
            // Including startPosition in the key forces a clean re-mount when
            // we navigate to a different saved offset (which never happens
            // mid-render today, but future-proofs against subtle re-renders).
            key={`${hlsSrc}#${startPosition.toFixed(0)}`}
            title={fileName}
            src={hlsSrc}
            autoPlay
            className="h-full w-full"
            onTimeUpdate={(detail) => {
              if (detail.currentTime > 0) {
                lastTimeRef.current = detail.currentTime;
              }
              if (
                infohash &&
                detail.currentTime > 5 &&
                detail.currentTime - lastSavedTimeRef.current > 7
              ) {
                lastSavedTimeRef.current = detail.currentTime;
                const dur = lastDurationRef.current ?? null;
                const completed = dur != null && detail.currentTime >= dur - 30;
                void progressApi.put(infohash, fileIdx, {
                  position_seconds: detail.currentTime,
                  duration_seconds: dur,
                  audio_track_idx: audioIdx ?? null,
                  subtitle_track_idx: subtitleTrackRef.current,
                  completed,
                });
              }
            }}
            onDurationChange={(detail) => {
              if (detail > 0) lastDurationRef.current = detail;
            }}
            onPause={() => {
              // Pause is the cheap moment to capture the exact position so
              // the next resume is pixel-precise (not throttled to 7s).
              if (!infohash) return;
              const t = lastTimeRef.current;
              if (t <= 0) return;
              lastSavedTimeRef.current = t;
              const dur = lastDurationRef.current ?? null;
              void progressApi.put(infohash, fileIdx, {
                position_seconds: t,
                duration_seconds: dur,
                audio_track_idx: audioIdx ?? null,
                subtitle_track_idx: subtitleTrackRef.current,
                completed: false,
              });
            }}
            onEnded={() => {
              if (!infohash) return;
              void progressApi.put(infohash, fileIdx, {
                position_seconds: lastDurationRef.current ?? lastTimeRef.current,
                duration_seconds: lastDurationRef.current,
                audio_track_idx: audioIdx ?? null,
                subtitle_track_idx: subtitleTrackRef.current,
                completed: true,
              });
            }}
            onCanPlay={() => {
              if (pendingSeek != null && playerRef.current) {
                try {
                  playerRef.current.currentTime = pendingSeek;
                } catch {
                  // The interval-driven effect will retry.
                }
                setPendingSeek(null);
              }
            }}
            onError={(detail) => {
              setPlayerError(
                `${detail.message ?? "unknown error"}` +
                  (detail.mediaError ? ` (code ${detail.mediaError.code})` : ""),
              );
            }}
            onProviderChange={(provider) => {
              if (isHLSProvider(provider)) {
                provider.config = {
                  ...provider.config,
                  // We pre-validated ENDLIST via the status endpoint, so
                  // these timeouts are pure defense in depth.
                  manifestLoadingTimeOut: 30_000,
                  manifestLoadingMaxRetry: 3,
                  manifestLoadingRetryDelay: 1000,
                  levelLoadingTimeOut: 30_000,
                  levelLoadingMaxRetry: 3,
                  fragLoadingTimeOut: 60_000,
                  fragLoadingMaxRetry: 6,
                  // Tell hls.js to load straight from the saved position
                  // instead of starting at 0 and us seeking into mid-buffer.
                  // Avoids "reprend à 0" + AppleVTDecoder errors on first try.
                  startPosition: startPosition > 0 ? startPosition : -1,
                };
              }
            }}
            ref={(p) => {
              playerRef.current = p;
            }}
          >
            <MediaProvider>
              {textSubs.map((s, i, all) => (
                <Track
                  key={String(s.index)}
                  src={torrents.subtitleUrl(infohash, fileIdx, s.index)}
                  kind="subtitles"
                  label={uniqueSubtitleLabel(s, i, all)}
                  lang={s.language ?? "und"}
                  default={s.default}
                />
              ))}
            </MediaProvider>
            <DefaultVideoLayout icons={defaultLayoutIcons} />
          </MediaPlayer>
        ) : (
          <PlayerLoadingStatus
            torrent={data}
            probeFetching={probeQ.isFetching}
            probeError={probeQ.error}
            progressPending={progressQ.isPending}
            audioReady={true}
            hlsStatus={hlsStatusQ.data ?? null}
            hlsError={hlsStatusQ.error}
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

      {probe && (probe.audio.length > 1 || probe.subtitle.length > 0) && (
        <div className="grid gap-3 rounded-md border border-border bg-card/40 p-4 text-sm">
          {probe.audio.length > 1 && (
            <AudioPicker
              audio={probe.audio}
              current={audioIdx ?? probe.audio.find((a) => a.default)?.index ?? probe.audio[0]?.index ?? 0}
              onPick={(i) => {
                if (i === audioIdx) return;
                // Master playlist exposes every audio rendition as
                // EXT-X-MEDIA, so we switch via hls.js's `audioTrack`
                // property — no URL change, no re-segmentation, no
                // re-buffering of video. Audio segments swap on the next
                // fragment boundary.
                const provider = playerRef.current?.provider;
                if (provider && isHLSProvider(provider) && provider.instance) {
                  provider.instance.audioTrack = i;
                }
                setAudioIdx(i);
                setPlayerError(null);
              }}
            />
          )}
          {probe.subtitle.length > 0 && (
            <div className="grid gap-1 text-xs">
              <span className="uppercase tracking-wide text-muted-foreground">
                Subtitles ({textSubs.length} loadable / {probe.subtitle.length} detected)
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
                      {!s.text_based && (
                        <span className="ml-1 text-amber-400">
                          (image-based, not exposable as WebVTT)
                        </span>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
              <span className="text-[11px]">Switch from the player CC menu.</span>
            </div>
          )}
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

function AudioPicker({
  audio,
  current,
  onPick,
}: {
  audio: AudioStream[];
  current: number;
  onPick: (idx: number) => void;
}) {
  return (
    <div className="grid gap-2">
      <span className="text-xs uppercase tracking-wide text-muted-foreground">Audio</span>
      <div className="flex flex-wrap gap-2">
        {audio.map((a) => {
          const active = a.index === current;
          return (
            <button
              key={a.index}
              type="button"
              onClick={() => onPick(a.index)}
              className={`rounded-md border px-3 py-1.5 text-xs transition ${
                active
                  ? "border-foreground bg-foreground text-background"
                  : "border-border hover:border-border/60"
              }`}
            >
              <span className="font-medium">{audioLabel(a)}</span>
              <span className="ml-2 text-[10px] uppercase opacity-70">
                {a.codec}
                {a.channels ? ` · ${a.channels}ch` : ""}
              </span>
              {!a.browser_compatible && (
                <Badge
                  variant="outline"
                  className="ml-2 border-amber-500/50 text-[9px] text-amber-300"
                >
                  transcode
                </Badge>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function audioLabel(a: AudioStream): string {
  if (a.title) return a.title;
  if (a.language) return a.language.toUpperCase();
  return `Audio ${a.index + 1}`;
}

function uniqueSubtitleLabel(s: SubtitleStream, _idx: number, all: SubtitleStream[]): string {
  const baseTitle = s.title?.trim();
  const langCode = s.language?.toUpperCase();
  const sameLang = all.filter((x) => x.language === s.language);
  const hasDuplicates = sameLang.length > 1 && !baseTitle;
  let label = baseTitle ?? langCode ?? `Sub ${s.index + 1}`;
  if (s.forced) label += " · forced";
  if (s.codec && s.codec.toLowerCase() !== "subrip") {
    label += ` · ${s.codec.toLowerCase()}`;
  }
  if (hasDuplicates) {
    const positionAmongSameLang = sameLang.findIndex((x) => x.index === s.index) + 1;
    label += ` (${positionAmongSameLang})`;
  }
  return label;
}

function tracingNoop(_: unknown) {
  // Swallowed: seeks can fail mid-buffer; we'll retry on the next interval tick.
}

function PlayerLoadingStatus({
  torrent,
  probeFetching,
  probeError,
  progressPending,
  audioReady,
  hlsStatus,
  hlsError,
}: {
  torrent: TorrentView;
  probeFetching: boolean;
  probeError: unknown;
  progressPending: boolean;
  audioReady: boolean;
  hlsStatus: import("@/lib/api").HlsStatus | null;
  hlsError: unknown;
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
  } else if (progressPending) {
    step = {
      label: "Loading saved position…",
    };
  } else if (!audioReady) {
    step = {
      label: "Selecting audio track…",
    };
  } else if (hlsError instanceof Error) {
    isError = true;
    step = { label: "Playback prep failed", sub: hlsError.message };
  } else if (!hlsStatus) {
    step = { label: "Starting transcoder…" };
  } else if (!hlsStatus.endlist_present) {
    const total = hlsStatus.estimated_total_segments;
    const seg = hlsStatus.segments_produced;
    const pct = total && total > 0 ? Math.min(99, (seg / total) * 100) : undefined;
    step = {
      label: total
        ? `Pre-segmenting · ${seg} / ~${total} segments`
        : `Pre-segmenting · ${seg} segments`,
      sub: "ffmpeg is writing the HLS playlist. Seek will be enabled once it finishes.",
      pct,
    };
  } else {
    step = {
      label: "Loading first frames…",
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
        <span
          className={`text-sm font-medium ${
            isError ? "text-destructive" : "text-foreground"
          }`}
        >
          {step.label}
        </span>
        {step.sub && (
          <span className="text-xs text-muted-foreground">{step.sub}</span>
        )}
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
