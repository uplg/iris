import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Container } from "@/components/Container";
import { HistoryList } from "@/components/HistoryList";
import { me, torrents, type HistoryItem } from "@/lib/api";

// The list is virtualized, so there's no rendering reason to paginate —
// just ask for the backend's max in one shot.
const LIMIT = 200;

/**
 * The caller's own full watch history, grouped by collection — including
 * "ghost" collections whose every torrent has since been reclaimed (they
 * stay listed under their clean title + poster; see `HistoryList`).
 * Distinct from the home page's "Continue watching" shelf, which only
 * shows unfinished items and drops deleted ones entirely — this is the
 * dedicated "where was I" answer after a cleanup: headers navigate to
 * the collection page (re-grab from the indexer offers), and GC'd rows
 * offer "Download again" (same release → same infohash → the saved
 * position resumes untouched).
 */
export function HistoryPage() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ["history"],
    queryFn: () => me.history(LIMIT, 0),
  });

  const [restoringKey, setRestoringKey] = useState<string | null>(null);
  const restore = useMutation({
    // `canRestore` in HistoryList guarantees both source fields are set.
    // Restoring a specific past release is explicit intent — skip the
    // duplicate-movie guard.
    mutationFn: (it: HistoryItem) =>
      torrents.ingest(it.source_provider!, it.source_external_id!, it.tmdb_id, true),
    onSuccess: (_res, it) => {
      void qc.invalidateQueries({ queryKey: ["history"] });
      void qc.invalidateQueries({ queryKey: ["library"] });
      // Straight back into playback: the stream path serves while the
      // torrent downloads, and the stored progress row picks the resume
      // position up — "reprend exactement où il était".
      void navigate({
        to: "/watch/$infohash/$idx",
        params: { infohash: it.infohash, idx: String(it.file_idx) },
      });
    },
    onSettled: () => setRestoringKey(null),
  });

  return (
    <Container>
      <div className="grid gap-8">
        <header className="grid gap-1.5">
          <span className="eyebrow">Profile</span>
          <h1 className="display" style={{ fontSize: "clamp(36px, 5vw, 56px)" }}>
            Watch history
          </h1>
        </header>

        {isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : !data?.length ? (
          <p className="text-sm text-muted-foreground">Nothing watched yet.</p>
        ) : (
          <HistoryList
            items={data}
            onPlay={(it) =>
              navigate({
                to: "/watch/$infohash/$idx",
                params: { infohash: it.infohash, idx: String(it.file_idx) },
              })
            }
            onOpenCollection={(id) => navigate({ to: "/collection/$id", params: { id } })}
            onRestore={(it) => {
              setRestoringKey(`${it.infohash}:${it.file_idx}`);
              restore.mutate(it as HistoryItem);
            }}
            restoringKey={restoringKey}
          />
        )}
      </div>
    </Container>
  );
}
