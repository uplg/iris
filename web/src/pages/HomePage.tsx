import { useQuery, keepPreviousData } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { ChevronLeft, ChevronRight, Film, Layers, Play, Search as SearchIcon, Tv } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Poster } from "@/components/Poster";
import {
  providers as providersApi,
  search,
  type MediaKind,
  type SearchResult,
  type SortField,
  type SortOrder,
} from "@/lib/api";
import { formatRelative, formatSize } from "@/lib/format";
import { PreviewDialog } from "@/components/PreviewDialog";
import { ContinueWatching } from "@/components/ContinueWatching";

const PAGE_SIZE = 25;

function useDebounce<T>(value: T, delay = 300): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}

export function HomePage() {
  const [q, setQ] = useState("");
  const debounced = useDebounce(q.trim(), 350);
  const [picked, setPicked] = useState<SearchResult | null>(null);
  const [page, setPage] = useState(1);
  const [sortBy, setSortBy] = useState<SortField>("seeders");
  const [order, setOrder] = useState<SortOrder>("desc");
  const [kind, setKind] = useState<MediaKind | null>(null);

  // Reset to page 1 when query, sort field, order or kind changes.
  useEffect(() => {
    setPage(1);
  }, [debounced, sortBy, order, kind]);

  const providersQ = useQuery({
    queryKey: ["providers"],
    queryFn: providersApi.list,
    staleTime: 5 * 60_000,
  });

  const { data, isFetching, error } = useQuery({
    queryKey: ["search", debounced, page, sortBy, order, kind],
    queryFn: () =>
      search.query(debounced, {
        page,
        limit: PAGE_SIZE,
        sort_by: sortBy,
        order,
        kind: kind ?? undefined,
      }),
    enabled: debounced.length >= 2,
    placeholderData: keepPreviousData,
  });

  const results = data?.results ?? [];
  // The server-side sort is what we trust; in case of multi-provider fan-out
  // future-proof a stable client-side tiebreaker by seeders desc when sort_by
  // doesn't match the resulting column.
  const rows = results;

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

  const onSort = (field: SortField) => {
    if (sortBy === field) {
      setOrder(order === "asc" ? "desc" : "asc");
    } else {
      setSortBy(field);
      setOrder(field === "title" ? "asc" : "desc");
    }
  };

  return (
    <div className="grid gap-6">
      <ContinueWatching />
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
              : providersQ.isLoading
                ? "Loading providers…"
                : providersQ.data && providersQ.data.length > 0
                  ? `${providersQ.data.length} provider${providersQ.data.length > 1 ? "s" : ""} ready: ${providersQ.data.map((p) => p.id).join(", ")}`
                  : "No search providers are configured."}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative min-w-0 flex-1">
            <SearchIcon className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              autoFocus
              placeholder="Title, year, anything…"
              className="h-12 pl-9 text-base"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
          </div>
          <ToggleGroup
            type="single"
            value={kind ?? "all"}
            onValueChange={(v) =>
              setKind(v === "movie" || v === "tv" ? v : null)
            }
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
            <ToggleGroupItem value="tv" aria-label="TV only">
              <Tv className="size-4" />
              TV
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </section>

      {error && (
        <p className="text-sm text-destructive">
          Search failed: {error instanceof Error ? error.message : String(error)}
        </p>
      )}

      {debounced.length < 2 ? (
        <p className="text-sm text-muted-foreground">Type at least 2 characters.</p>
      ) : isFetching && rows.length === 0 ? (
        <p className="text-sm text-muted-foreground">Searching…</p>
      ) : rows.length === 0 ? (
        <p className="text-sm text-muted-foreground">No results.</p>
      ) : (
        <>
          <div className="overflow-hidden rounded-lg border border-border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[60px]"></TableHead>
                  <SortableTh
                    label="Title"
                    field="title"
                    sortBy={sortBy}
                    order={order}
                    onSort={onSort}
                    className="w-full"
                  />
                  <TableHead>Provider</TableHead>
                  <TableHead>Category</TableHead>
                  <SortableTh
                    label="Size"
                    field="size"
                    sortBy={sortBy}
                    order={order}
                    onSort={onSort}
                    className="text-right"
                  />
                  <SortableTh
                    label="S"
                    field="seeders"
                    sortBy={sortBy}
                    order={order}
                    onSort={onSort}
                    className="text-right"
                  />
                  <SortableTh
                    label="L"
                    field="leechers"
                    sortBy={sortBy}
                    order={order}
                    onSort={onSort}
                    className="text-right"
                  />
                  <SortableTh
                    label="Uploaded"
                    field="uploaded"
                    sortBy={sortBy}
                    order={order}
                    onSort={onSort}
                  />
                  <TableHead></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r) => (
                  <TableRow
                    key={`${r.provider_id}:${r.external_id}`}
                    className="cursor-pointer"
                    onClick={() => setPicked(r)}
                  >
                    <TableCell className="align-top">
                      <Poster
                        tmdbId={r.tmdb_id}
                        kind={r.kind}
                        size="sm"
                        alt={r.title}
                      />
                    </TableCell>
                    <TableCell className="max-w-0 align-top">
                      <div className="truncate font-medium" title={r.title}>
                        {r.title}
                      </div>
                      {(r.year || r.freeleech || r.tags.length > 0) && (
                        <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                          {r.year && <span>{r.year}</span>}
                          {r.freeleech && (
                            <Badge
                              variant="secondary"
                              className="bg-emerald-500/10 text-[10px] uppercase text-emerald-400"
                            >
                              FL
                            </Badge>
                          )}
                          {r.tags.slice(0, 4).map((t) => (
                            <span key={t} className="rounded bg-muted/40 px-1 py-0.5 text-[10px]">
                              {t}
                            </span>
                          ))}
                        </div>
                      )}
                    </TableCell>
                    <TableCell className="align-top">
                      <Badge variant="outline" className="text-[10px] uppercase">
                        {r.provider_id}
                      </Badge>
                    </TableCell>
                    <TableCell className="align-top text-xs text-muted-foreground">
                      {r.category ?? "—"}
                    </TableCell>
                    <TableCell className="align-top text-right tabular-nums">
                      {formatSize(r.size_bytes)}
                    </TableCell>
                    <TableCell className="align-top text-right tabular-nums text-emerald-400">
                      {r.seeders ?? 0}
                    </TableCell>
                    <TableCell className="align-top text-right tabular-nums text-rose-400">
                      {r.leechers ?? 0}
                    </TableCell>
                    <TableCell className="align-top whitespace-nowrap text-xs text-muted-foreground">
                      {r.uploaded_at ? formatRelative(r.uploaded_at) : "—"}
                    </TableCell>
                    <TableCell
                      className="align-top text-right"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Button size="sm" onClick={() => setPicked(r)}>
                        <Play className="size-3.5" />
                        Play
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
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

type SortableThProps = {
  label: string;
  field: SortField;
  sortBy: SortField;
  order: SortOrder;
  onSort: (f: SortField) => void;
  className?: string;
};

function SortableTh({ label, field, sortBy, order, onSort, className }: SortableThProps) {
  const active = sortBy === field;
  const indicator = !active ? "" : order === "asc" ? "▲" : "▼";
  return (
    <TableHead className={className}>
      <button
        type="button"
        className={`flex items-center gap-1 ${active ? "text-foreground" : "text-muted-foreground hover:text-foreground"} ${
          className?.includes("text-right") ? "ml-auto" : ""
        }`}
        onClick={() => onSort(field)}
      >
        {label}
        <span className="text-[10px]">{indicator}</span>
      </button>
    </TableHead>
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
          Prev
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
