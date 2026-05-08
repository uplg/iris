import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { Progress } from "@/components/ui/progress";
import { me } from "@/lib/api";

export function ContinueWatching() {
  const { data, isLoading } = useQuery({
    queryKey: ["continue-watching"],
    queryFn: me.continueWatching,
    staleTime: 30_000,
  });

  if (isLoading) return null;
  if (!data || data.length === 0) return null;

  return (
    <section className="grid gap-3">
      <h2 className="text-xs uppercase tracking-wide text-muted-foreground">Continue watching</h2>
      <ul className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {data.map((it) => {
          const pct =
            it.duration_seconds && it.duration_seconds > 0
              ? Math.min(100, (it.position_seconds / it.duration_seconds) * 100)
              : 0;
          // For multi-file releases, the file path tells which episode;
          // fall back to the torrent name otherwise.
          const fileName = it.file_path ? it.file_path.split("/").pop() : null;
          const primary = fileName ?? it.torrent_name;
          const secondary = fileName && fileName !== it.torrent_name ? it.torrent_name : null;
          return (
            <li
              key={`${it.infohash}:${it.file_idx}`}
              className="rounded-lg border border-border bg-card/40 p-3 transition hover:border-border/60 hover:bg-card/70"
            >
              <Link to={`/watch/${it.infohash}/${it.file_idx}`} className="grid gap-1.5">
                <span className="line-clamp-2 break-words text-sm font-medium" title={primary}>
                  {primary}
                </span>
                {secondary && (
                  <span
                    className="line-clamp-1 break-words text-[11px] text-muted-foreground"
                    title={secondary}
                  >
                    {secondary}
                  </span>
                )}
                <Progress value={pct} className="h-1" />
                <span className="text-[11px] text-muted-foreground">
                  {pct > 0
                    ? `${pct.toFixed(0)}% · ${formatTimecode(it.position_seconds)}`
                    : "Just started"}
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function formatTimecode(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
