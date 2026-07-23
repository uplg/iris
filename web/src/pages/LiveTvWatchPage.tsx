import { useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { ArrowLeft, Shuffle } from "lucide-react";

import { livetv } from "@/lib/api";
import { IrisPlayer } from "@/lib/iris-core/IrisPlayer";
import type { DecodeTier, Manifest } from "@/lib/iris-core/manifest-client";
import { readStoredVolume, writeStoredVolume } from "@/lib/player-volume";
import { programmeProgress } from "@/pages/LiveTvPage";

/** Faster than the grid (30 s): the overlay shows a live progress bar. */
const EPG_REFETCH_MS = 30_000;

/** Auto source-rotations before we give up and show the failure banner.
 *  "Try another source" resets the budget (fresh player section). */
const MAX_AUTO_ROTATIONS = 2;

const noop = () => {
  /* live: no progress to persist */
};

/** Minimal Manifest for a live channel. IrisPlayer is manifest-driven but a
 *  live stream has no probed tracks — the engines discover codecs from the
 *  HLS playlists at mount. Empty track lists keep the chrome's subtitle and
 *  audio menus hidden; a null duration marks the stream endless. */
function liveManifest(channelName: string): Manifest {
  return {
    schema_version: 1,
    infohash: "live",
    file_idx: 0,
    filename: channelName,
    container: "hls",
    size_bytes: 0,
    duration_s: null,
    download: { bytes_complete: 0, progress: 1, ranges_complete: [] },
    header_byte_range: { start: 0, end: 0 },
    index_at_end: false,
    moov_at_start: null,
    tail_byte_range: null,
    video: [],
    audio: [],
    subtitles: [],
    chapters: [],
  };
}

export function LiveTvWatchPage() {
  const navigate = useNavigate();
  const { country = "fr", channelId = "" } = useParams({ strict: false }) as {
    country?: string;
    channelId?: string;
  };

  const channelsQ = useQuery({
    queryKey: ["livetv", "channels", country],
    queryFn: () => livetv.channels(country),
    staleTime: 10 * 60 * 1000,
  });
  const epgQ = useQuery({
    queryKey: ["livetv", "epg-now", country],
    queryFn: () => livetv.epgNow(country),
    refetchInterval: EPG_REFETCH_MS,
  });

  const channel = channelsQ.data?.channels.find((c) => c.id === channelId);
  const nowNext = useMemo(
    () => epgQ.data?.entries.find((e) => e.channel_id === channelId),
    [epgQ.data, channelId],
  );
  const now = nowNext?.now ?? null;
  const next = nowNext?.next ?? null;
  const progress = now ? programmeProgress(now.start, now.stop) : null;

  const name = channel?.name ?? channelId;
  // Bumped by "Try another source" — rebuilds the player section from
  // scratch (fresh rotation budget, fresh probe).
  const [playerEpoch, setPlayerEpoch] = useState(0);

  return (
    <div className="mx-auto grid w-full max-w-[1280px] gap-4 px-4 sm:px-6 lg:px-8">
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => navigate({ to: "/live", search: { country } })}
          className="focus-ring inline-flex items-center gap-1.5 rounded-full border border-border px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="size-4" /> Channels
        </button>
        <h1 className="truncate font-display text-xl font-semibold">{name}</h1>
        {channel?.geo_blocked && (
          <span className="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">
            May be geo-blocked
          </span>
        )}
        <div className="flex-1" />
        {/* Escape hatch for "it plays but badly" (garbled sound, artifacts):
            the player can't detect that itself, but the viewer can. Reports
            the current feed (backend cools it down, elects the next one)
            and rebuilds the player. */}
        <button
          type="button"
          onClick={() => {
            void livetv.reportPlaybackError(country, channelId).catch(() => {});
            setPlayerEpoch((n) => n + 1);
          }}
          className="focus-ring inline-flex shrink-0 items-center gap-1.5 rounded-full border border-border px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground"
        >
          <Shuffle className="size-4" /> Try another source
        </button>
      </div>

      <LivePlayerSection
        key={`${country}:${channelId}:${playerEpoch}`}
        country={country}
        channelId={channelId}
        channelName={name}
      />

      {(now || next) && (
        <div className="grid gap-2 rounded-xl border border-border bg-elev p-4">
          {now && (
            <div className="grid gap-1.5">
              <div className="flex items-baseline justify-between gap-3">
                <span className="truncate font-medium">{now.title}</span>
                <span className="shrink-0 text-xs text-muted-foreground">
                  {formatTime(now.start)} – {formatTime(now.stop)}
                </span>
              </div>
              {progress != null && (
                <span className="h-1 overflow-hidden rounded-full bg-border">
                  <span
                    className="block h-full rounded-full bg-red-500/80"
                    style={{ width: `${progress}%` }}
                  />
                </span>
              )}
              {now.description && (
                <p className="line-clamp-2 text-sm text-muted-foreground">{now.description}</p>
              )}
            </div>
          )}
          {next && (
            <p className="text-sm text-muted-foreground">
              <span className="text-foreground/70">Up next · {formatTime(next.start)}</span>{" "}
              {next.title}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The player itself: IrisPlayer in live mode. Engine choice is driven by
 * WHICH source the backend elected — one probe GET on the master reads the
 * `x-iris-live-upstream` header (the fetch also warms the election + the
 * tuner's remux session server-side):
 *
 *   - `tuner` → Tier B live (mediabunny demux of the fMP4 remux; libav
 *     decodes the broadcast E-AC-3 client-side). hls.js CANNOT play that
 *     feed — a muxed fMP4 with `ec-3` in its codec string is rejected by
 *     MSE wholesale.
 *   - anything else → Tier F live (hls.js, battle-tested against dirty
 *     internet restreams, + the WebAudio sidecar for E-AC-3-in-TS feeds).
 *
 * On a player error we report the feed (the backend demotes it and elects
 * the next one), re-probe, and remount — bounded by MAX_AUTO_ROTATIONS.
 */
function LivePlayerSection(props: { country: string; channelId: string; channelName: string }) {
  const { country, channelId, channelName } = props;
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  const rotations = useRef(0);
  const manifest = useMemo(() => liveManifest(channelName), [channelName]);
  const masterUrl = livetv.masterUrl(country, channelId);

  const probeQ = useQuery({
    queryKey: ["livetv", "probe", country, channelId, attempt],
    queryFn: async (): Promise<{ tier: DecodeTier }> => {
      const res = await fetch(masterUrl, { credentials: "include" });
      if (!res.ok) throw new Error(`master fetch failed (${res.status})`);
      const upstream = res.headers.get("x-iris-live-upstream");
      // Tuner feed (broadcast -c copy, open-GOP H.264 + E-AC-3) plays via
      // the WebCodecs live engine — client-side decode with our own
      // broadcast concealment; MSE's in-browser decoders kill the pipeline
      // on mid-stream joins. Browsers without WebCodecs (iOS Safari) take
      // Tier F: the native HLS pipeline decodes E-AC-3 + deinterlaces on
      // Apple hardware.
      const webcodecs = typeof globalThis.VideoDecoder !== "undefined";
      return { tier: upstream === "tuner" && webcodecs ? "C" : "F" };
    },
    staleTime: 0,
    gcTime: 0,
    retry: 1,
  });

  // Failure policy in two stages: the FIRST failure remounts the SAME
  // source silently (a mount hiccup — a 401, a join timeout — is not
  // evidence the source is bad, and reporting it sends the tuner into an
  // escalating cooldown that strands the next probes on internet feeds:
  // the "dead until force-refresh" spiral). Only a repeat failure reports
  // the source and rotates.
  const softRetried = useRef(false);
  const rotate = (reason: string) => {
    if (!softRetried.current) {
      softRetried.current = true;
      console.warn(`[live] remounting same source after: ${reason}`);
      setAttempt((n) => n + 1);
      return;
    }
    softRetried.current = false;
    console.warn(`[live] rotating source: ${reason}`);
    void livetv.reportPlaybackError(country, channelId).catch(() => {
      /* best-effort */
    });
    if (rotations.current < MAX_AUTO_ROTATIONS) {
      rotations.current += 1;
      setAttempt((n) => n + 1);
    } else {
      setFailed(true);
    }
  };

  // `?r=` forces IrisPlayer's mount effect to re-fire on rotation (the
  // backend ignores unknown query params on the master route).
  const src = attempt > 0 ? `${masterUrl}?r=${attempt}` : masterUrl;
  const showFailed = failed || probeQ.isError;

  return (
    <div className="relative aspect-video w-full overflow-hidden rounded-xl border border-border bg-black">
      {probeQ.data && !failed && (
        <IrisPlayer
          live
          tier={probeQ.data.tier}
          src={src}
          srcType="application/vnd.apple.mpegurl"
          title={channelName}
          manifest={manifest}
          startPosition={0}
          initialVolume={readStoredVolume()}
          onVolumeChange={(v) => writeStoredVolume(v)}
          onTimeUpdate={noop}
          onDurationChange={noop}
          onSeeking={noop}
          onPause={noop}
          onEnded={() => rotate("stream ended")}
          onError={(message) => rotate(message)}
        />
      )}
      {showFailed && (
        <div className="absolute inset-0 grid place-items-center bg-black/80 p-6 text-center">
          <div className="grid justify-items-center gap-3">
            <p className="font-medium text-white">Stream unavailable</p>
            <p className="max-w-sm text-sm text-white/70">
              {channelName} isn't reachable right now. The channel may be geo-blocked, offline, or
              its source is down.
            </p>
            <button
              type="button"
              className="focus-ring rounded-full border border-white/25 px-4 py-1.5 text-sm text-white hover:bg-white/10"
              onClick={() => {
                rotations.current = 0;
                setFailed(false);
                setAttempt((n) => n + 1);
              }}
            >
              Retry
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
