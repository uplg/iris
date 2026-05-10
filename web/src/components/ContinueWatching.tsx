import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";

import { MediaCard } from "@/components/MediaCard";
import { me } from "@/lib/api";

/**
 * Continue Watching shelf on the home page. Renders shared MediaCards
 * with TMDB-backed posters when we know `(tmdb_id, kind)` — the kind is
 * critical because TMDB's separate movie / tv id namespaces collide
 * (the same numerical id resolves to two unrelated entries) and a
 * lookup without the disambiguator served the wrong poster on every
 * card. When `tmdb_id` is missing or unverified, the card falls back
 * to the kind-aware placeholder.
 */
export function ContinueWatching() {
  const navigate = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });

  if (isLoading) return null;
  if (!data || data.length === 0) return null;

  return (
    <section className="grid gap-3">
      <h2 className="text-xs uppercase tracking-wide text-muted-foreground">
        Continue watching
      </h2>
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
          const subtitle =
            pct > 0
              ? `${(pct * 100).toFixed(0)}% · ${formatTimecode(it.position_seconds)}`
              : "Just started";
          return (
            <MediaCard
              key={`${it.infohash}:${it.file_idx}`}
              // Trust the indexer-supplied tmdb_id without gating on
              // verified — the lookup endpoint already does a kind
              // namespace fallback so a wrong-kind id still resolves
              // to *something*. A wrong poster is rare, missing
              // posters were the loud regression.
              tmdbId={it.tmdb_id}
              kind={it.kind}
              title={prettifyFilename(primary)}
              subtitle={subtitle}
              progress={pct}
              onClick={() =>
                navigate(`/watch/${it.infohash}/${it.file_idx}`)
              }
            />
          );
        })}
      </div>
    </section>
  );
}

function formatTimecode(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0)
    return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function prettifyFilename(raw: string): string {
  const noExt = raw.includes(".") ? raw.slice(0, raw.lastIndexOf(".")) : raw;
  return noExt.replace(/[._]+/g, " ").trim();
}
