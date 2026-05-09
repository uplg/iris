import { useEffect } from "react";
import { useNavigate, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Loader2, Play } from "lucide-react";

import { Button } from "@/components/ui/button";
import { library } from "@/lib/api";

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

/**
 * Fallback page for collections that don't route directly to /series
 * (TV with no TMDB id) or /watch (movie with TMDB → Series page handles
 * it). Shows what we have and lets the user pick a file.
 *
 * Design choice: this page exists for the long-tail edge cases
 * (untagged TV, movies without TMDB). The common path is handled by
 * `LibraryPage` routing collections to `/series/:tmdb_id` or directly
 * to `/watch` when there's a single video file.
 */
export function CollectionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data, isLoading, error } = useQuery({
    queryKey: ["collection", id],
    queryFn: () => library.collection(id!),
    enabled: !!id,
  });

  // Auto-navigate cases: a movie collection with one video → straight
  // to /watch. Saves a useless intermediate click.
  useEffect(() => {
    if (!data) return;
    if (data.kind === "movie") {
      const t = data.torrents[0];
      const f = t?.files
        .filter((x) => VIDEO_RE.test(x.path))
        .sort((a, b) => b.size_bytes - a.size_bytes)[0];
      if (t && f) {
        navigate(`/watch/${t.infohash}/${f.index}`, { replace: true });
      }
    } else if (data.kind === "tv" && data.tmdb_id) {
      // Same intent as the LibraryPage routing logic — if we landed
      // here despite having a tmdb_id, take the user where the rich
      // Series UI lives.
      navigate(`/series/${data.tmdb_id}`, { replace: true });
    }
  }, [data, navigate]);

  if (isLoading) {
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-3 animate-spin" />
        Chargement de la collection…
      </p>
    );
  }
  if (error) {
    return (
      <p className="text-sm text-destructive">
        {error instanceof Error ? error.message : "failed"}
      </p>
    );
  }
  if (!data) return null;

  return (
    <div className="grid gap-6">
      <header>
        <h1 className="text-3xl font-semibold tracking-tight">{data.display_title}</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {data.kind === "tv" ? "Série" : "Film"} · {data.torrents.length} torrent
          {data.torrents.length > 1 ? "s" : ""}
        </p>
      </header>

      {data.kind === "tv" && data.episodes.length > 0 ? (
        <ul className="divide-y divide-border rounded-lg border border-border bg-card/30">
          {data.episodes.map((e) => (
            <li
              key={`${e.infohash}:${e.file_idx}`}
              className="flex items-center justify-between gap-3 px-4 py-3 text-sm"
            >
              <span className="font-mono text-muted-foreground">
                S{e.season.toString().padStart(2, "0")}E
                {e.episode.toString().padStart(2, "0")}
              </span>
              {e.watched && (
                <span className="text-xs text-emerald-300">vu</span>
              )}
              <Button asChild size="sm" className="ml-auto">
                <a href={`/watch/${e.infohash}/${e.file_idx}`}>
                  <Play className="size-3.5" />
                  {e.watched ? "Revoir" : "Lire"}
                </a>
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-muted-foreground">
          Pas d'épisode tagué pour cette collection. Vue détaillée des
          torrents disponible dans /library?view=torrents.
        </p>
      )}
    </div>
  );
}
