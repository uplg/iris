import { useQuery } from "@tanstack/react-query";
import { Film, Tv } from "lucide-react";
import { metadata, tmdbImage, type MediaKind } from "@/lib/api";

type Size = "xs" | "sm" | "md" | "lg";

const SIZE_CLASSES: Record<Size, string> = {
  xs: "h-12 w-8",
  sm: "h-16 w-11",
  md: "h-24 w-16",
  lg: "h-40 w-28",
};

const TMDB_SIZE: Record<Size, "w92" | "w154" | "w185" | "w342"> = {
  xs: "w92",
  sm: "w92",
  md: "w154",
  lg: "w342",
};

/**
 * Poster image fetched from TMDB by id. Falls back to a placeholder when:
 * - No `tmdbId` (e.g. random Mangas-Animes torrent without TMDB mapping)
 * - TMDB lookup 404s
 * - `IRIS_TMDB__API_KEY` is not configured server-side
 *
 * Lookups are cached forever in TanStack Query — TMDB metadata is essentially
 * static so re-fetching is wasted bandwidth.
 */
export function Poster({
  tmdbId,
  kind,
  size = "sm",
  alt,
}: {
  tmdbId: number | null | undefined;
  kind?: MediaKind | null;
  size?: Size;
  alt?: string;
}) {
  const { data } = useQuery({
    // `kind` disambiguates TMDB's movie/tv namespaces — same numerical
    // id resolves to two unrelated entries otherwise.
    queryKey: ["tmdb", tmdbId, kind],
    queryFn: () => metadata.tmdb(tmdbId!, kind ?? undefined),
    enabled: tmdbId != null,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });

  const url = tmdbImage(data?.poster_path, TMDB_SIZE[size]);
  const wrapper =
    "shrink-0 overflow-hidden rounded-md border border-border bg-muted/40";

  if (url) {
    return (
      <div className={`${wrapper} ${SIZE_CLASSES[size]}`}>
        <img
          src={url}
          alt={alt ?? data?.title ?? ""}
          loading="lazy"
          className="h-full w-full object-cover"
        />
      </div>
    );
  }

  const Icon = kind === "tv" ? Tv : Film;
  return (
    <div
      className={`${wrapper} ${SIZE_CLASSES[size]} flex items-center justify-center text-muted-foreground/60`}
    >
      <Icon className="size-5" />
    </div>
  );
}
