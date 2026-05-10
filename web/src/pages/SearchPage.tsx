import { useQuery, keepPreviousData } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import {
  ChevronLeft,
  ChevronRight,
  Film,
  Layers,
  Loader2,
  Search as SearchIcon,
  Tv,
} from "lucide-react";

import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { PreviewDialog } from "@/components/PreviewDialog";
import { EmptyState, ErrorState, LoadingState } from "@/components/State";
import {
  metadata,
  search,
  tmdbImage,
  type MediaKind,
  type SearchResult,
  type SortField,
  type SortOrder,
  type TmdbSuggestion,
} from "@/lib/api";

/**
 * UI-level sort presets. `recommended` is a client-side composite
 * score (seeders ÷ √size_GiB) that surfaces fast-to-process releases
 * first — small files with healthy swarms, which is what most users
 * actually want. The remaining presets pass straight through to the
 * indexer's sort field with a fixed direction. Mirrors the same five
 * options on the Android TV `SearchScreen` so the experience matches.
 */
type SortMode = "recommended" | "seeders" | "uploaded" | "size" | "title";

const SORT_MODES: readonly { id: SortMode; label: string }[] = [
  { id: "recommended", label: "Recommended" },
  { id: "seeders", label: "Seeders" },
  { id: "uploaded", label: "Newest" },
  { id: "size", label: "Smallest" },
  { id: "title", label: "Title" },
] as const;

function apiSort(mode: SortMode): { sort_by: SortField; order: SortOrder } {
  switch (mode) {
    case "recommended":
    case "seeders":
      return { sort_by: "seeders", order: "desc" };
    case "uploaded":
      return { sort_by: "uploaded", order: "desc" };
    case "size":
      return { sort_by: "size", order: "asc" };
    case "title":
      return { sort_by: "title", order: "asc" };
  }
}

/**
 * Composite score for `recommended`. `seeders / √size_GiB` favours
 * small + popular releases without crushing every chunky encode out
 * of the top picks. Falls back to raw seeders when size is unknown so
 * usable hits don't sink to the bottom.
 */
function recommendedScore(r: SearchResult): number {
  const seeders = r.seeders ?? 0;
  if (r.size_bytes && r.size_bytes > 0) {
    const sizeGiB = r.size_bytes / (1024 ** 3);
    if (sizeGiB > 0.1) return seeders / Math.sqrt(sizeGiB);
  }
  return seeders;
}
import { formatRelative, formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";

const PAGE_SIZE = 25;

function useDebounce<T>(value: T, delay = 300): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}

/**
 * Search page with TMDB-driven typeahead. As the user types we hit the
 * backend's TMDB-multi-search proxy (debounced 250 ms) and surface
 * suggestions in a dropdown — picking one runs the indexer search with
 * the canonical TMDB title (cleaner queries → better tracker matches).
 * Manual queries still work as before.
 *
 * Results render as a grid of cards (poster + title + metadata + tags),
 * not the legacy table. Click a card → PreviewDialog with the rich
 * description / NFO breakdown / "ingest" CTA.
 */
export function SearchPage() {
  const [params, setParams] = useSearchParams();
  const initialQ = params.get("q") ?? "";
  const [q, setQ] = useState(initialQ);
  const debounced = useDebounce(q.trim(), 350);
  const [picked, setPicked] = useState<SearchResult | null>(null);
  const [page, setPage] = useState(1);
  const [sortMode, setSortMode] = useState<SortMode>("recommended");
  const [kind, setKind] = useState<MediaKind | null>(null);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Mirror the in-input value into the URL so refresh keeps the search
  // and copy/paste of the URL replays it.
  useEffect(() => {
    if (debounced.length >= 2) {
      setParams({ q: debounced }, { replace: true });
    } else if (debounced.length === 0 && params.get("q")) {
      setParams({}, { replace: true });
    }
  }, [debounced, params, setParams]);

  // Reset to page 1 when query/sort/kind changes.
  useEffect(() => {
    setPage(1);
  }, [debounced, sortMode, kind]);

  // ---------- TMDB typeahead ----------
  const typeaheadQ = useDebounce(q.trim(), 250);
  const suggestionsQ = useQuery({
    queryKey: ["tmdb-suggest", typeaheadQ],
    queryFn: () => metadata.tmdbSearch(typeaheadQ),
    enabled: typeaheadQ.length >= 2 && showSuggestions,
    staleTime: 60_000,
  });

  // ---------- Indexer search ----------
  const { sort_by, order } = apiSort(sortMode);
  const { data, isFetching, error } = useQuery({
    queryKey: ["search", debounced, page, sortMode, kind],
    queryFn: async () => {
      const res = await search.query(debounced, {
        page,
        limit: PAGE_SIZE,
        sort_by,
        order,
        kind: kind ?? undefined,
      });
      // `Recommended` re-ranks the seeders-desc page client-side using
      // a composite score. Other modes pass straight through.
      if (sortMode === "recommended") {
        return {
          ...res,
          results: [...res.results].sort(
            (a, b) => recommendedScore(b) - recommendedScore(a),
          ),
        };
      }
      return res;
    },
    enabled: debounced.length >= 2,
    placeholderData: keepPreviousData,
  });


  const rows = data?.results ?? [];
  const meta = data?.providers ?? [];
  const totals = useMemo(() => {
    let count = 0;
    let pages = 0;
    for (const p of meta) {
      if (p.total_count) count += p.total_count;
      if (p.total_pages && p.total_pages > pages) pages = p.total_pages;
    }
    return { count, pages };
  }, [meta]);

  const onPickSuggestion = (s: TmdbSuggestion) => {
    setQ(s.title);
    setShowSuggestions(false);
    if (s.kind === "movie" || s.kind === "tv") setKind(s.kind);
    inputRef.current?.blur();
  };

  return (
    <div className="grid gap-6">
      <section className="grid gap-4">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">Search</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {meta.length > 0
              ? meta
                  .map((p) =>
                    p.error
                      ? `${p.id} (unavailable)`
                      : `${p.id}${p.total_count != null ? ` (${p.total_count.toLocaleString()})` : ""}`,
                  )
                  .join(" · ")
              : "Search a title — TMDB suggestions appear as you type."}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative min-w-0 flex-1">
            <SearchIcon className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={inputRef}
              autoFocus
              placeholder="Title, year, anything…"
              className="h-12 pl-9 text-base"
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                setShowSuggestions(true);
              }}
              onFocus={() => setShowSuggestions(true)}
              onBlur={() => {
                // Delay so a click on a suggestion can land before we hide.
                window.setTimeout(() => setShowSuggestions(false), 150);
              }}
            />
            <SuggestionsDropdown
              open={
                showSuggestions &&
                typeaheadQ.length >= 2 &&
                ((suggestionsQ.data?.length ?? 0) > 0 || suggestionsQ.isFetching)
              }
              loading={suggestionsQ.isFetching}
              items={suggestionsQ.data ?? []}
              onPick={onPickSuggestion}
            />
          </div>
          <ToggleGroup
            type="single"
            value={kind ?? "all"}
            onValueChange={(v) => setKind(v === "movie" || v === "tv" ? v : null)}
            className="shrink-0"
          >
            <ToggleGroupItem value="all" aria-label="All categories">
              <Layers className="size-4" />
              All
            </ToggleGroupItem>
            <ToggleGroupItem value="movie" aria-label="Movies only">
              <Film className="size-4" />
              Movies
            </ToggleGroupItem>
            <ToggleGroupItem value="tv" aria-label="Series only">
              <Tv className="size-4" />
              Series
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <span>Sort</span>
          {SORT_MODES.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => setSortMode(m.id)}
              className={cn(
                "rounded-md px-2 py-0.5",
                sortMode === m.id
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:bg-muted/40",
              )}
            >
              {m.label}
            </button>
          ))}
        </div>
      </section>

      {error && <ErrorState title="Search failed" error={error} />}

      {debounced.length < 2 ? (
        <EmptyState
          icon={<SearchIcon className="size-7" />}
          title="Type at least 2 characters"
          body="Tip: pick a TMDB suggestion to use a canonical title — better results from the tracker."
        />
      ) : isFetching && rows.length === 0 ? (
        <LoadingState label="Searching…" />
      ) : rows.length === 0 ? (
        <EmptyState
          title="No results"
          body="Try a different title, drop the year, or switch between Movies / Series in the filter."
        />
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
            {rows.map((r) => (
              <ResultCard
                key={`${r.provider_id}:${r.external_id}`}
                result={r}
                onClick={() => setPicked(r)}
              />
            ))}
          </div>

          <Pagination
            page={page}
            totalPages={totals.pages || 1}
            onPage={setPage}
            isFetching={isFetching}
          />
        </>
      )}

      <PreviewDialog
        open={picked != null}
        onOpenChange={(o) => !o && setPicked(null)}
        providerId={picked?.provider_id ?? null}
        externalId={picked?.external_id ?? null}
        initialTitle={picked?.title}
        tmdbId={picked?.tmdb_id ?? null}
      />
    </div>
  );
}

/**
 * Extract the canonical name from a SCENE-style release title. Walks
 * tokens (split on `.`, `_`, space, `(`, `[`) and stops at the first
 * "metadata" token — a year, SxxExx, resolution, source, codec or
 * language tag. Whatever's before the stop token is the title.
 *
 * Examples:
 *   "Silicon.Valley.S01E01.1080p.BluRay.x264-XYZ" → "Silicon Valley"
 *   "The.Burning.Bed.1984.DVDRip.x264-XYZ"        → "The Burning Bed"
 *   "Avatar.2009.1080p.BluRay.HEVC-Group"         → "Avatar"
 *   "Game of Thrones - Season 1 - 1080p"          → "Game of Thrones"
 *
 * Falls back to the raw title when nothing parses (e.g. user-uploaded
 * names without standard separators).
 */
const STOP_TOKEN = new RegExp(
  [
    "^\\d{4}$", // year
    "^s\\d{1,2}(e\\d{1,3})?$", // S01 / S01E01
    "^e\\d{1,3}$", // E01
    "^season$",
    "^(480p|576p|720p|1080p|1440p|2160p|4k|uhd)$",
    "^(bluray|brrip|bdrip|webrip|web-?dl|web|hdtv|hdrip|dvdrip|hdlight|remux|hr-hdtv)$",
    "^(x264|x265|h\\.?264|h\\.?265|hevc|avc|av1|xvid|divx)$",
    "^(french|truefrench|vff|vfi|vfq|vf|vostfr|multi|english|eng|vo|vost)$",
    "^(complete|repack|proper|extended|directors?|uncut|hdr|hdr10|dv)$",
  ].join("|"),
  "i",
);

function extractSceneTitle(raw: string): string {
  // Drop "Various filename punctuation" → spaces, then split.
  const tokens = raw
    .replace(/[._\-[\]()]+/g, " ")
    .split(/\s+/)
    .filter(Boolean);
  const head: string[] = [];
  for (const t of tokens) {
    if (STOP_TOKEN.test(t)) break;
    head.push(t);
  }
  const cleaned = head.join(" ").trim();
  return cleaned.length >= 2 ? cleaned : raw.trim();
}

function SuggestionsDropdown({
  open,
  loading,
  items,
  onPick,
}: {
  open: boolean;
  loading: boolean;
  items: TmdbSuggestion[];
  onPick: (s: TmdbSuggestion) => void;
}) {
  if (!open) return null;
  return (
    <div className="absolute left-0 right-0 top-full z-30 mt-1 max-h-96 overflow-y-auto rounded-md border border-border bg-popover shadow-lg">
      {loading && items.length === 0 && (
        <div className="flex items-center gap-2 px-3 py-2 text-xs text-muted-foreground">
          <Loader2 className="size-3 animate-spin" />
          TMDB suggestions…
        </div>
      )}
      {items.map((s) => (
        <button
          key={`${s.kind}-${s.tmdb_id}`}
          type="button"
          onClick={() => onPick(s)}
          className="flex w-full items-center gap-3 border-t border-border/40 px-3 py-2 text-left first:border-t-0 hover:bg-muted/40"
        >
          {s.poster_path ? (
            <img
              src={tmdbImage(s.poster_path, "w92") ?? undefined}
              alt=""
              className="h-12 w-8 shrink-0 rounded-sm object-cover"
              loading="lazy"
            />
          ) : (
            <div className="flex h-12 w-8 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground/60">
              {s.kind === "tv" ? <Tv className="size-4" /> : <Film className="size-4" />}
            </div>
          )}
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium">
              {s.title}
              {s.year && (
                <span className="ml-1.5 text-xs text-muted-foreground">({s.year})</span>
              )}
            </div>
            {s.overview && (
              <div className="truncate text-[11px] text-muted-foreground">{s.overview}</div>
            )}
          </div>
          <Badge variant="outline" className="text-[10px] uppercase">
            {s.kind}
          </Badge>
        </button>
      ))}
    </div>
  );
}

function ResultCard({
  result,
  onClick,
}: {
  result: SearchResult;
  onClick: () => void;
}) {
  // Resolve the poster by *title* rather than by indexer-supplied
  // `tmdb_id`. The latter is wrong often enough that we've stopped
  // trusting it for visual cues (torr9 mislabels e.g. Silicon Valley
  // releases with The Burning Bed's id). The cleaned SCENE title
  // comes from the release name itself, which is the authoritative
  // identifier on every tracker.
  const cleaned = useMemo(() => extractSceneTitle(result.title), [result.title]);
  const tmdbHitQ = useQuery({
    queryKey: ["tmdb-by-title", cleaned, result.kind ?? "any"],
    queryFn: async () => {
      const hits = await metadata.tmdbSearch(cleaned);
      // Prefer a hit matching the result's kind when known.
      return (result.kind ? hits.find((h) => h.kind === result.kind) : null)
        ?? hits[0]
        ?? null;
    },
    enabled: cleaned.length >= 2,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });
  // Last-resort fallback: a pre-resolved poster URL the indexer
  // sometimes ships (featured items on torr9). We use it only when
  // TMDB resolution genuinely failed.
  const poster =
    tmdbImage(tmdbHitQ.data?.poster_path, "w342") ?? result.poster_url ?? null;
  const Icon = result.kind === "tv" ? Tv : Film;
  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-3 text-left transition hover:border-border/80 hover:bg-card/70"
    >
      <div className="relative aspect-[2/3] overflow-hidden rounded-md bg-muted/40">
        {poster ? (
          <img
            src={poster}
            alt={result.title}
            loading="lazy"
            className="h-full w-full object-cover transition group-hover:scale-[1.02]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-gradient-to-b from-primary/15 to-background/80 text-muted-foreground/60">
            <Icon className="size-8" />
          </div>
        )}
        <div className="absolute right-1.5 top-1.5 flex flex-col items-end gap-1">
          {result.freeleech && (
            <Badge className="bg-emerald-500/90 text-[10px] uppercase text-white shadow-md">
              FL
            </Badge>
          )}
          <Badge variant="secondary" className="text-[10px] uppercase shadow-md">
            {result.provider_id}
          </Badge>
        </div>
      </div>
      <div className="grid gap-1">
        <div className="line-clamp-2 text-sm font-medium leading-tight" title={result.title}>
          {result.title}
        </div>
        {(result.year || tmdbHitQ.data?.year) && (
          <div className="text-[11px] text-muted-foreground">
            {result.year ?? tmdbHitQ.data?.year}
          </div>
        )}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
          <span className="text-emerald-400">{result.seeders ?? 0} ↑</span>
          <span className="text-rose-400">{result.leechers ?? 0} ↓</span>
          <span>{formatSize(result.size_bytes)}</span>
          {result.uploaded_at && <span>{formatRelative(result.uploaded_at)}</span>}
        </div>
        {result.tags.length > 0 && (
          <div className="mt-1 flex flex-wrap gap-1">
            {result.tags.slice(0, 4).map((t) => (
              <span
                key={t}
                className="rounded bg-muted/40 px-1.5 py-0.5 text-[10px] text-muted-foreground"
              >
                {t}
              </span>
            ))}
          </div>
        )}
      </div>
    </button>
  );
}

type PaginationProps = {
  page: number;
  totalPages: number;
  onPage: (n: number) => void;
  isFetching: boolean;
};

function Pagination({ page, totalPages, onPage, isFetching }: PaginationProps) {
  return (
    <div className="flex items-center justify-between gap-3 text-sm text-muted-foreground">
      <span>
        Page {page} of {totalPages || 1}
        {isFetching && <span className="ml-2 text-xs">(loading…)</span>}
      </span>
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => onPage(Math.max(1, page - 1))}
          disabled={page <= 1 || isFetching}
        >
          <ChevronLeft className="size-3.5" />
          Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => onPage(page + 1)}
          disabled={page >= totalPages || isFetching}
        >
          Next
          <ChevronRight className="size-3.5" />
        </Button>
      </div>
    </div>
  );
}
