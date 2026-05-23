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

import { useNavigate } from "react-router";

import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { LanguageBadge } from "@/components/LanguageBadge";
import { PreviewDialog } from "@/components/PreviewDialog";
import { EmptyState, ErrorState, LoadingState } from "@/components/State";
import {
  metadata,
  search,
  tmdbImage,
  type MediaKind,
  type ParsedQueryInfo,
  type SearchResult,
  type SortField,
  type SortOrder,
  type TmdbSuggestion,
} from "@/lib/api";

/**
 * UI-level sort presets. `relevance` is the default: it lets the
 * backend's ranker order results by SCENE-parsed title + S/E match,
 * plus a seeders/√size_GiB quality bonus. The other modes pass
 * straight through to the indexer's sort field with a fixed
 * direction. Mirrors the same five options on the Android TV
 * `SearchScreen` so the experience matches.
 */
type SortMode = "relevance" | "seeders" | "uploaded" | "size" | "title";

const SORT_MODES: readonly { id: SortMode; label: string }[] = [
  { id: "relevance", label: "Relevance" },
  { id: "seeders", label: "Seeders" },
  { id: "uploaded", label: "Newest" },
  { id: "size", label: "Smallest" },
  { id: "title", label: "Title" },
] as const;

function apiSort(mode: SortMode): {
  sort_by?: SortField;
  order?: SortOrder;
} {
  // `relevance` is conveyed by *not* sending `sort_by`: the backend
  // owns the ranking in that case (title/SE match + quality
  // composite + library-dedup demotion).
  switch (mode) {
    case "relevance":
      return {};
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

import { formatRelative, formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";

/** "S04E11" / "S04 pack" / "Season 4" depending on which parts the
 *  SCENE parser extracted. Episode 0 is the in-band season-pack
 *  sentinel — render it as "pack" rather than the meaningless E00. */
function formatSceneMarker(season: number, episode: number | null | undefined): string {
  const s = `S${String(season).padStart(2, "0")}`;
  if (episode == null) return s;
  if (episode === 0) return `${s} · Season pack`;
  return `${s}E${String(episode).padStart(2, "0")}`;
}

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
  const [sortMode, setSortMode] = useState<SortMode>("relevance");
  const [kind, setKind] = useState<MediaKind | null>(null);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const navigate = useNavigate();

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
  // The server is the source of truth for ordering. In `relevance`
  // mode it sends nothing back through `sort_by`, leaving the
  // backend's parser-aware ranker (title/SxxExx + quality +
  // library-dedup demotion) in charge. The other sort modes pass
  // straight through.
  const { sort_by, order } = apiSort(sortMode);
  const { data, isFetching, error } = useQuery({
    queryKey: ["search", debounced, page, sortMode, kind],
    queryFn: () =>
      search.query(debounced, {
        page,
        limit: PAGE_SIZE,
        sort_by,
        order,
        kind: kind ?? undefined,
      }),
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
          {/* Own row below sm so the suggestions popover (anchored
              left-0/right-0 to this box) spans the full width and is
              usable — otherwise the ToggleGroup shares the row and
              squeezes the input to a sliver. `basis-full` (not w-full)
              is what actually forces the wrap: on a flex item `flex-1`
              pins flex-basis to 0% and overrides `width`, so we set the
              basis directly. `sm:basis-0` restores the shared row. */}
          <div className="relative grow basis-full sm:min-w-0 sm:basis-0">
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

      {data?.parsed_query && <ParsedQueryBanner info={data.parsed_query} rawQuery={debounced} />}

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
                onPlayExisting={(infohash, fileIdx) => navigate(`/watch/${infohash}/${fileIdx}`)}
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
        alreadyInLibrary={picked?.already_in_library ?? false}
        libraryInfohash={picked?.library_infohash ?? null}
        libraryFileIdx={picked?.library_file_idx ?? null}
      />
    </div>
  );
}

/** Renders the SCENE-style summary the server attached to the
 *  response (title + season + episode + year) above the results
 *  grid. Reassures the user that the indexer was queried with the
 *  structured fields, not the raw string. */
function ParsedQueryBanner({ info, rawQuery }: { info: ParsedQueryInfo; rawQuery: string }) {
  const parts: string[] = [];
  if (info.season != null) {
    if (info.episode != null && info.episode > 0) {
      parts.push(
        `S${String(info.season).padStart(2, "0")}E${String(info.episode).padStart(2, "0")}`,
      );
    } else {
      parts.push(`Season ${info.season}`);
    }
  }
  if (info.year != null) {
    parts.push(String(info.year));
  }
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-border bg-card/30 px-3 py-2 text-xs text-muted-foreground">
      <span>
        Showing results for{" "}
        <span className="font-medium capitalize text-foreground">{info.title}</span>
        {parts.length > 0 && (
          <>
            {" "}
            · <span className="font-mono text-foreground">{parts.join(" · ")}</span>
          </>
        )}
      </span>
      <span className="hidden text-muted-foreground sm:inline" title={rawQuery}>
        Parsed from your query
      </span>
    </div>
  );
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
              {s.year && <span className="ml-1.5 text-xs text-muted-foreground">({s.year})</span>}
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
  onPlayExisting,
}: {
  result: SearchResult;
  onClick: () => void;
  onPlayExisting: (infohash: string, fileIdx: number) => void;
}) {
  // Resolve the poster by *release name* rather than by indexer-supplied
  // `tmdb_id`. The latter is wrong often enough that we've stopped
  // trusting it for visual cues (torr9 mislabels e.g. Silicon Valley
  // releases with The Burning Bed's id). We send the untouched title to
  // `/tmdb/resolve`, which parses + scores it by kind + year server-side
  // (one source of truth, shared 30d cache) instead of the old
  // client-side "clean title → popularity #1" heuristic that mismatched
  // short titles like "Pride" → "Pride and Prejudice".
  const tmdbHitQ = useQuery({
    queryKey: ["tmdb-resolve", result.title, result.kind ?? "any"],
    queryFn: () => metadata.tmdbResolve(result.title, result.kind),
    enabled: result.title.length >= 2,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });
  // Last-resort fallback: a pre-resolved poster URL the indexer
  // sometimes ships (featured items on torr9). We use it only when
  // TMDB resolution genuinely failed.
  const poster = tmdbImage(tmdbHitQ.data?.poster_path, "w342") ?? result.poster_url ?? null;
  const Icon = result.kind === "tv" ? Tv : Film;
  const owned = result.already_in_library === true;
  const canPlayExisting =
    owned && result.library_infohash != null && result.library_file_idx != null;
  return (
    <button
      type="button"
      onClick={() => {
        // One-click play when we can resolve a direct watch URL,
        // otherwise fall back to the preview dialog (which renders
        // a clearer "you already own this — download anyway?" UI).
        if (canPlayExisting) {
          onPlayExisting(result.library_infohash!, result.library_file_idx!);
        } else {
          onClick();
        }
      }}
      className={cn(
        "group flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-3 text-left transition hover:border-border/80 hover:bg-card/70",
        owned && "ring-1 ring-emerald-500/40",
      )}
    >
      <div className="relative aspect-[2/3] overflow-hidden rounded-md bg-muted/40">
        {poster ? (
          <img
            src={poster}
            alt={result.title}
            loading="lazy"
            className={cn(
              "h-full w-full object-cover transition group-hover:scale-[1.02]",
              owned && "opacity-80",
            )}
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-gradient-to-b from-primary/15 to-background/80 text-muted-foreground/60">
            <Icon className="size-8" />
          </div>
        )}
        {owned && (
          // Top-left "already in library" pill. Distinct corner from
          // FL/provider so the two never overlap.
          <Badge className="absolute left-1.5 top-1.5 bg-emerald-500/95 text-[10px] uppercase text-white shadow-md">
            In library
          </Badge>
        )}
        <div className="absolute right-1.5 top-1.5 flex flex-col items-end gap-1">
          <LanguageBadge language={result.language} />
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
      <div className="grid gap-1.5">
        {/* SCENE-parsed (S, E) up top — the most scannable piece of
            metadata on a TV release. Users can spot their episode
            without parsing the full release name with their eyes. */}
        {result.parsed_season != null && (
          <div className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-primary">
            {formatSceneMarker(result.parsed_season, result.parsed_episode)}
          </div>
        )}
        {/* Title clamped to 3 lines to keep the grid visually
            consistent (cards stay roughly the same height instead
            of ballooning on long SCENE names). The S/E chip above
            does the heavy lifting for "find my episode" scanning;
            `title=` provides the full string on hover when the
            user needs the remaining tokens. `break-words` keeps
            dot-separated SCENE names from overflowing horizontally
            on a narrow card. */}
        <div
          className="line-clamp-3 break-words text-sm font-medium leading-snug"
          title={result.title}
        >
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
