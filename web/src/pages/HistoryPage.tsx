import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Container } from "@/components/Container";
import { HistoryList } from "@/components/HistoryList";
import { me } from "@/lib/api";

// The list is virtualized, so there's no rendering reason to paginate —
// just ask for the backend's max in one shot.
const LIMIT = 200;

/**
 * The caller's own full watch history — in-progress and completed, one row
 * per episode, including titles whose source torrent has since been
 * deleted (see `HistoryList`). Distinct from the home page's "Continue
 * watching" shelf, which only shows unfinished items and drops deleted
 * ones entirely — this is the dedicated "where was I" answer after a
 * cleanup.
 */
export function HistoryPage() {
  const navigate = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["history"],
    queryFn: () => me.history(LIMIT, 0),
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
          />
        )}
      </div>
    </Container>
  );
}
