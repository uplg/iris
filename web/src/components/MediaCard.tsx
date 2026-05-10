import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { Film, Tv } from "lucide-react";
import type { ReactNode } from "react";

import { metadata, tmdbImage, type MediaKind } from "@/lib/api";
import { cn } from "@/lib/utils";

type Size = "sm" | "md" | "lg";

const SIZE_CLASSES: Record<Size, string> = {
  sm: "w-32",
  md: "w-40",
  lg: "w-48",
};

const TMDB_SIZE: Record<Size, "w185" | "w342" | "w500"> = {
  sm: "w185",
  md: "w342",
  lg: "w500",
};

export type MediaCardProps = {
  /** Direct poster URL. If not given AND `tmdbId` is, looks up via TMDB
   *  metadata cache. If neither yields a URL, renders a kind-aware
   *  placeholder (Film / Tv icon). */
  posterUrl?: string | null;
  tmdbId?: number | null;
  /** Drives the placeholder icon when no poster is available. */
  kind?: MediaKind | null;
  /** Primary line under the poster. */
  title: string;
  /** Secondary line; truncates to one line. */
  subtitle?: string;
  /** Top-right overlay (e.g., "3 new", "freeleech", a status pill).
   *  Render-as-is — caller picks the styling. */
  badge?: ReactNode;
  /** 0..1 — when set, draws a thin progress bar across the bottom of the
   *  poster. Color comes from `progressColor`. */
  progress?: number;
  /** Tailwind text-color class, e.g. `bg-primary` for watch progress or
   *  `bg-sky-500` for downloads. Defaults to primary. */
  progressColor?: string;
  /** When set, the whole card becomes a `<Link>`. Mutually exclusive
   *  with `onClick` (link wins). */
  href?: string;
  onClick?: () => void;
  size?: Size;
  className?: string;
};

/**
 * Single shared card for every shelf — Continue Watching, Watchlist,
 * Featured, Library, Search results. Replaces the half-dozen ad-hoc card
 * shapes that used to live inline in each page. Poster comes from a
 * direct URL or a TMDB lookup, optional progress bar + top-right badge,
 * caption below.
 */
export function MediaCard(props: MediaCardProps) {
  const {
    posterUrl,
    tmdbId,
    kind,
    title,
    subtitle,
    badge,
    progress,
    progressColor = "bg-primary",
    href,
    onClick,
    size = "md",
    className,
  } = props;

  // TMDB lookup only fires when no direct posterUrl is provided. Lookups
  // are cached forever (TMDB metadata is essentially static for an id).
  const tmdbQ = useQuery({
    queryKey: ["tmdb", tmdbId],
    queryFn: () => metadata.tmdb(tmdbId!),
    enabled: tmdbId != null && !posterUrl,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });

  const finalUrl = posterUrl ?? tmdbImage(tmdbQ.data?.poster_path, TMDB_SIZE[size]);
  const Icon = kind === "tv" ? Tv : Film;

  const inner = (
    <div className={cn(SIZE_CLASSES[size], "flex flex-col gap-2", className)}>
      <div className="relative aspect-[2/3] overflow-hidden rounded-lg border border-border bg-muted/40 transition group-hover:border-border/80 group-focus-visible:ring-2 group-focus-visible:ring-ring">
        {finalUrl ? (
          <img
            src={finalUrl}
            alt={title}
            loading="lazy"
            className="h-full w-full object-cover transition group-hover:scale-[1.02]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-gradient-to-b from-primary/20 to-background/80 text-muted-foreground/60">
            <Icon className="size-8" />
          </div>
        )}

        {badge && (
          <div className="absolute right-1.5 top-1.5">
            {badge}
          </div>
        )}

        {progress != null && progress > 0 && (
          <>
            <div className="absolute inset-x-0 bottom-0 h-1 bg-black/40" />
            <div
              className={cn("absolute bottom-0 left-0 h-1", progressColor)}
              style={{ width: `${Math.min(100, Math.max(0, progress * 100))}%` }}
            />
          </>
        )}
      </div>

      <div className="grid gap-0.5 px-0.5">
        <span
          className="line-clamp-2 break-words text-sm font-medium leading-tight"
          title={title}
        >
          {title}
        </span>
        {subtitle && (
          <span className="line-clamp-1 break-words text-[11px] text-muted-foreground">
            {subtitle}
          </span>
        )}
      </div>
    </div>
  );

  // The `group` class is applied to the outer interactive element so the
  // child hover/focus styles (poster border, scale) all activate from a
  // single focus boundary — matters for keyboard nav.
  if (href) {
    return (
      <Link
        to={href}
        className="group block rounded-lg outline-none focus-visible:outline-none"
      >
        {inner}
      </Link>
    );
  }
  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        className="group block rounded-lg text-left outline-none focus-visible:outline-none"
      >
        {inner}
      </button>
    );
  }
  return <div className="group block">{inner}</div>;
}
