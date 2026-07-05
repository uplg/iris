import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Check, MoreVertical, X } from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { me, progress } from "@/lib/api";
import { formatTimecode } from "@/lib/format";

/**
 * Continue Watching shelf on the home page. Renders shared MediaCards
 * with TMDB-backed posters when we know `(tmdb_id, kind)` — the server
 * already falls back from `collection.tmdb_id` to `torrent.tmdb_id` via
 * `COALESCE`, and the `/api/metadata/tmdb/{id}` lookup tries both
 * `/movie/X` and `/tv/X` so a wrong-kind hint still resolves. When
 * neither tmdb_id is set we render the kind-aware placeholder.
 */
export function ContinueWatching() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });

  const refresh = () => void qc.invalidateQueries({ queryKey: ["continue-watching"] });
  const remove = (it: { collection_id?: string | null; infohash: string; file_idx: number }) =>
    void me
      .dismissContinueWatching(
        it.collection_id
          ? { collection_id: it.collection_id }
          : { infohash: it.infohash, file_idx: it.file_idx },
      )
      .then(refresh);
  const markWatched = (infohash: string, idx: number) =>
    void progress.markWatched(infohash, idx).then(refresh);

  if (isLoading) return null;
  if (!data || data.length === 0) return null;

  return (
    <section className="grid gap-3">
      <h2 className="text-xs uppercase tracking-wide text-muted-foreground">Continue watching</h2>
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {data.map((it) => {
          const pct =
            it.duration_seconds && it.duration_seconds > 0
              ? Math.min(1, it.position_seconds / it.duration_seconds)
              : 0;
          // For multi-file releases, the file path tells which episode;
          // fall back to the torrent name otherwise.
          const fileName = it.file_path ? it.file_path.split("/").pop() : null;
          const primary = fileName ?? it.torrent_name;
          const subtitle = it.next_up
            ? "Up next"
            : pct > 0
              ? `${(pct * 100).toFixed(0)}% · ${formatTimecode(it.position_seconds)}`
              : "Just started";
          return (
            <div key={`${it.infohash}:${it.file_idx}`} className="group relative">
              <MediaCard
                tmdbId={it.tmdb_id}
                kind={it.kind ?? null}
                title={primary}
                subtitle={subtitle}
                progress={pct}
                onClick={() =>
                  navigate({
                    to: "/watch/$infohash/$idx",
                    params: { infohash: it.infohash, idx: String(it.file_idx) },
                  })
                }
              />
              {/* Manage menu — remove / mark watched. Shown on hover/focus. */}
              <DropdownMenu>
                <DropdownMenuTrigger
                  aria-label="Manage"
                  onClick={(e) => e.stopPropagation()}
                  className="focus-ring absolute top-2 right-2 grid size-8 place-items-center rounded-full bg-black/60 text-white opacity-0 backdrop-blur transition-opacity group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                >
                  <MoreVertical className="size-4" />
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
                  <DropdownMenuItem onSelect={() => markWatched(it.infohash, it.file_idx)}>
                    <Check className="size-4" />
                    {it.next_up ? "Mark watched & skip" : "Mark as watched"}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => remove(it)}>
                    <X className="size-4" /> Remove from Continue Watching
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          );
        })}
      </div>
    </section>
  );
}
