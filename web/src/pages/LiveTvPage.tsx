import { useDeferredValue, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { Radio as LiveIcon } from "lucide-react";

import { Container } from "@/components/Container";
import { livetv, type LiveChannel, type LiveNowNext } from "@/lib/api";
import { logoTone, type LogoTone } from "@/lib/logo-tone";

/** Refetch cadence for the now/next strip — the guide only changes on
 *  programme boundaries, 60 s keeps progress bars honest without hammering. */
const EPG_REFETCH_MS = 60_000;

export function LiveTvPage() {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { country?: string };

  const countriesQ = useQuery({
    queryKey: ["livetv", "countries"],
    queryFn: () => livetv.countries(),
    staleTime: 24 * 60 * 60 * 1000,
  });
  const country = search.country ?? countriesQ.data?.default_country ?? "fr";

  const channelsQ = useQuery({
    queryKey: ["livetv", "channels", country],
    queryFn: () => livetv.channels(country),
    staleTime: 10 * 60 * 1000,
  });
  // Cross-country channel search (server-side). useDeferredValue keeps
  // typing snappy without a timer (banned in web/); the endpoint is an
  // in-memory index, per-keystroke queries are cheap.
  const [query, setQuery] = useState("");
  const deferredQ = useDeferredValue(query.trim());
  const searchQ = useQuery({
    queryKey: ["livetv", "search", deferredQ],
    queryFn: () => livetv.search(deferredQ),
    enabled: deferredQ.length >= 2,
    staleTime: 60_000,
  });

  const epgQ = useQuery({
    queryKey: ["livetv", "epg-now", country],
    queryFn: () => livetv.epgNow(country),
    refetchInterval: EPG_REFETCH_MS,
  });

  const epgByChannel = useMemo(() => {
    const map = new Map<string, LiveNowNext>();
    for (const e of epgQ.data?.entries ?? []) map.set(e.channel_id, e);
    return map;
  }, [epgQ.data]);

  const sections = useMemo(() => {
    const channels = channelsQ.data?.channels ?? [];
    const tnt = channels.filter((c) => c.tnt_number != null);
    const rest = channels.filter((c) => c.tnt_number == null);
    const byCategory = new Map<string, LiveChannel[]>();
    for (const c of rest) {
      const key = c.categories[0] ?? "Other";
      const bucket = byCategory.get(key) ?? [];
      bucket.push(c);
      byCategory.set(key, bucket);
    }
    return { tnt, categories: [...byCategory.entries()] };
  }, [channelsQ.data]);

  const openChannel = (c: LiveChannel) => {
    // Search results carry their own country as "cc:id" (see below);
    // regular grid channels use the picker's country.
    const [ctry, id] = c.id.includes(":") ? c.id.split(":", 2) : [country, c.id];
    navigate({
      to: "/live/$country/$channelId",
      params: { country: ctry, channelId: id },
    });
  };

  return (
    <Container>
      <div className="grid gap-8">
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div className="grid gap-1.5">
            <span className="eyebrow">Live</span>
            <h1 className="display" style={{ fontSize: "clamp(36px, 5vw, 56px)" }}>
              Live TV
            </h1>
          </div>
          <div className="flex flex-wrap items-center gap-4">
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search channels (all countries)…"
              className="focus-ring h-9 w-64 rounded-md border border-border bg-elev px-3 text-sm text-foreground placeholder:text-muted-foreground"
            />
          <label className="flex items-center gap-2 text-sm text-muted-foreground">
            Country
            <select
              className="focus-ring h-9 rounded-md border border-border bg-elev px-2 text-sm text-foreground"
              value={country}
              onChange={(e) => navigate({ to: "/live", search: { country: e.target.value } })}
            >
              {(countriesQ.data?.countries ?? []).map((c) => (
                <option key={c.code} value={c.code}>
                  {c.flag} {c.name}
                </option>
              ))}
            </select>
          </label>
          </div>
        </header>

        {deferredQ.length >= 2 ? (
          (searchQ.data?.results ?? []).length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {searchQ.isLoading ? "Searching…" : `No channel matches "${deferredQ}".`}
            </p>
          ) : (
            <ChannelSection
              title={`Results · ${searchQ.data?.results.length ?? 0}`}
              channels={(searchQ.data?.results ?? []).map((r) => ({
                // country smuggled through the id (slugs are alphanumeric,
                // ":" is safe) — unpacked by openChannel below.
                id: `${r.country}:${r.id}`,
                name: `${r.name} · ${r.country.toUpperCase()}`,
                logo_url: r.logo_url ?? null,
                logo_origin: r.logo_origin ?? null,
                categories: [],
                geo_blocked: false,
                not_24_7: false,
                quality: null,
                tnt_number: null,
              }))}
              epg={epgByChannel}
              onOpen={openChannel}
            />
          )
        ) : channelsQ.isLoading ? (
          <p className="text-sm text-muted-foreground">Loading channels…</p>
        ) : channelsQ.isError ? (
          <p className="text-sm text-muted-foreground">
            Couldn't load channels for this country — the upstream playlist may be unavailable.
          </p>
        ) : (
          <>
            {sections.tnt.length > 0 && (
              <ChannelSection
                title="TNT"
                channels={sections.tnt}
                epg={epgByChannel}
                onOpen={openChannel}
                showNumber
              />
            )}
            {sections.categories.map(([title, channels]) => (
              <ChannelSection
                key={title}
                title={title}
                channels={channels}
                epg={epgByChannel}
                onOpen={openChannel}
              />
            ))}
            {!sections.tnt.length && !sections.categories.length && (
              <p className="text-sm text-muted-foreground">No channels available.</p>
            )}
          </>
        )}
      </div>
    </Container>
  );
}

function ChannelSection({
  title,
  channels,
  epg,
  onOpen,
  showNumber,
}: {
  title: string;
  channels: LiveChannel[];
  epg: Map<string, LiveNowNext>;
  onOpen: (c: LiveChannel) => void;
  showNumber?: boolean;
}) {
  return (
    <section className="grid gap-3">
      <h2 className="font-display text-lg font-semibold">{title}</h2>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {channels.map((c) => (
          <ChannelCard
            key={c.id}
            channel={c}
            nowNext={epg.get(c.id)}
            onOpen={() => onOpen(c)}
            showNumber={showNumber}
          />
        ))}
      </div>
    </section>
  );
}

function ChannelCard({
  channel,
  nowNext,
  onOpen,
  showNumber,
}: {
  channel: LiveChannel;
  nowNext?: LiveNowNext;
  onOpen: () => void;
  showNumber?: boolean;
}) {
  const now = nowNext?.now ?? null;
  const progress = now ? programmeProgress(now.start, now.stop) : null;

  return (
    <button
      type="button"
      onClick={onOpen}
      className="focus-ring group grid gap-2 rounded-xl border border-border bg-elev p-3 text-left transition-colors hover:border-foreground/25"
    >
      {/* Adaptive "logo well": channel logos are wild PNGs — black ink
          vanishes on a dark plate, white ink on a light one. The plate
          color is picked from the logo's own luminance (see logo-tone.ts):
          dark logo → light well, light logo → dark well, colorful → gray. */}
      <ChannelLogoWell channel={channel}>
        {showNumber && channel.tnt_number != null && (
          <span className="absolute top-1 left-1 rounded bg-black/65 px-1.5 py-0.5 font-mono text-[11px] text-white">
            {channel.tnt_number}
          </span>
        )}
        {channel.quality != null && (
          <span className="absolute top-1 right-1 rounded bg-black/65 px-1.5 py-0.5 text-[10px] text-white/80">
            {channel.quality}p
          </span>
        )}
      </ChannelLogoWell>
      <div className="grid gap-1">
        <span className="truncate text-sm font-medium">{channel.name}</span>
        {now ? (
          <>
            <span className="flex items-center gap-1.5 truncate text-xs text-muted-foreground">
              <LiveIcon className="size-3 shrink-0 text-red-500" />
              <span className="truncate">{now.title}</span>
            </span>
            {progress != null && (
              <span className="h-1 overflow-hidden rounded-full bg-border">
                <span
                  className="block h-full rounded-full bg-red-500/80"
                  style={{ width: `${progress}%` }}
                />
              </span>
            )}
          </>
        ) : (
          <span className="truncate text-xs text-muted-foreground">
            {channel.geo_blocked ? "May be geo-blocked" : channel.not_24_7 ? "Not 24/7" : " "}
          </span>
        )}
      </div>
    </button>
  );
}

const WELL_CLASS: Record<LogoTone, string> = {
  light: "bg-white/90",
  neutral: "bg-zinc-400/50",
  dark: "bg-zinc-950/80",
};

/** Memoized per-URL tone as React state ("neutral" until analyzed). */
function useLogoTone(url: string | null): LogoTone {
  const [tone, setTone] = useState<LogoTone>(() => {
    if (!url) return "neutral";
    const hit = logoTone(url);
    return typeof hit === "string" ? hit : "neutral";
  });
  useEffect(() => {
    if (!url) return;
    const hit = logoTone(url);
    if (typeof hit === "string") {
      setTone(hit);
      return;
    }
    let cancelled = false;
    void hit.then((t) => {
      if (!cancelled) setTone(t);
    });
    return () => {
      cancelled = true;
    };
  }, [url]);
  return tone;
}

function ChannelLogoWell({
  channel,
  children,
}: {
  channel: LiveChannel;
  children?: React.ReactNode;
}) {
  // Logo URLs are same-origin backend-proxy paths (`/api/livetv/logo?…`) —
  // no CORS, no mixed content, and the luminance analysis can read pixels.
  // Broken/missing logos fall back to a letter tile via onError (swap
  // handled in plain DOM to avoid extra state).
  const url = channel.logo_url ?? null;
  const tone = useLogoTone(url);
  return (
    <div
      className={`relative flex h-16 items-center justify-center rounded-lg p-2 transition-colors ${WELL_CLASS[tone]}`}
    >
      {url ? (
        <>
          <img
            src={url}
            alt=""
            loading="lazy"
            referrerPolicy="no-referrer"
            className="max-h-12 max-w-full object-contain"
            onError={(e) => {
              e.currentTarget.style.display = "none";
              const sibling = e.currentTarget.nextElementSibling as HTMLElement | null;
              if (sibling) sibling.style.display = "grid";
            }}
          />
          <span style={{ display: "none" }}>
            <LetterTile name={channel.name} />
          </span>
        </>
      ) : (
        <LetterTile name={channel.name} />
      )}
      {children}
    </div>
  );
}

function LetterTile({ name }: { name: string }) {
  return (
    <span
      className="grid size-12 place-items-center rounded-lg font-display text-lg font-semibold text-primary-foreground"
      style={{ background: "linear-gradient(135deg, var(--brand-3), var(--brand))" }}
    >
      {name.charAt(0).toUpperCase()}
    </span>
  );
}

/** 0–100 position of "now" inside a programme, `null` outside its window. */
export function programmeProgress(startIso: string, stopIso: string): number | null {
  const start = Date.parse(startIso);
  const stop = Date.parse(stopIso);
  if (!Number.isFinite(start) || !Number.isFinite(stop) || stop <= start) return null;
  const pos = ((Date.now() - start) / (stop - start)) * 100;
  if (pos < 0 || pos > 100) return null;
  return Math.round(pos);
}
