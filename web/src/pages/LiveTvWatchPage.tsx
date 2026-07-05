import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { ArrowLeft, Shuffle } from "lucide-react";

import { LivePlayer } from "@/components/LivePlayer";
import { livetv } from "@/lib/api";
import { programmeProgress } from "@/pages/LiveTvPage";

/** Faster than the grid (30 s): the overlay shows a live progress bar. */
const EPG_REFETCH_MS = 30_000;

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

  const src = livetv.masterUrl(country, channelId);
  const name = channel?.name ?? channelId;
  // Bumped by "Try another source" — remounts the player from scratch.
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
            and remounts. */}
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

      <div className="aspect-video w-full overflow-hidden rounded-xl border border-border">
        <LivePlayer
          key={playerEpoch}
          src={src}
          channelName={name}
          country={country}
          channelId={channelId}
        />
      </div>

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

function formatTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
