import { useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Container } from "@/components/Container";
import { HistoryList } from "@/components/HistoryList";
import { admin } from "@/lib/api";

// The list is virtualized, so there's no rendering reason to paginate —
// just ask for the backend's max in one shot.
const LIMIT = 200;

const route = getRouteApi("/auth/shell/adminGuard/admin/users/$userId/history");

/**
 * Admin drill-down into one user's full watch history — same data shape
 * and rendering as the user-facing `HistoryPage`, reached via
 * `admin.userHistory()` instead of `me.history()`. Linked from the Users
 * card on `/admin`.
 */
export function AdminUserHistoryPage() {
  const { userId } = route.useParams();
  const navigate = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["admin", "user-history", userId],
    queryFn: () => admin.userHistory(userId, LIMIT, 0),
  });

  return (
    <Container>
      <div className="grid gap-8">
        <header className="flex flex-wrap items-end justify-between gap-4 border-b border-warn/30 pb-5">
          <div className="grid gap-1.5">
            <span className="eyebrow text-warn/80">Engine room</span>
            <h1 className="display" style={{ fontSize: "clamp(32px, 4.5vw, 48px)" }}>
              User history
            </h1>
          </div>
          <Button asChild variant="ghost" size="sm">
            <Link to="/admin">
              <ArrowLeft className="size-3.5" />
              Back to admin
            </Link>
          </Button>
        </header>

        {isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : !data?.length ? (
          <p className="text-sm text-muted-foreground">This user hasn't watched anything yet.</p>
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
