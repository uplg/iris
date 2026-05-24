import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

const BUILD_ID: string = __IRIS_BUILD_ID__;

/**
 * Detects a deploy and offers a (non-blocking) reload.
 *
 * The build emits `dist/version.json` carrying the build id; the backend
 * serves it `no-cache, must-revalidate`, so it always reflects the CURRENTLY
 * deployed bundle. We poll it and compare against the id baked into the
 * running bundle (`__IRIS_BUILD_ID__`). A mismatch means a newer bundle is
 * deployed while this tab still runs the old one → show a banner.
 *
 * The poll is driven by React Query's `refetchInterval` + `refetchOnWindowFocus`
 * (event-driven primitives, not a hand-rolled `setInterval`). The fetch
 * tolerates a 404 (dev, where the file isn't emitted) by reporting "no update".
 *
 * Unlike the `ClientOutdatedOverlay` (a hard 426 lock-out), this is advisory:
 * the user keeps using the old bundle — handy mid-playback — until they choose
 * to reload.
 */
export function UpdateBanner() {
  const { data: latestBuildId } = useQuery({
    queryKey: ["deploy-build-id"],
    queryFn: async () => {
      try {
        const res = await fetch("/version.json", { cache: "no-store" });
        if (!res.ok) return null;
        const body = (await res.json()) as { buildId?: string };
        return body.buildId ?? null;
      } catch {
        return null;
      }
    },
    refetchInterval: 5 * 60_000,
    refetchOnWindowFocus: true,
    retry: false,
    staleTime: 0,
  });

  const updateAvailable = Boolean(latestBuildId && latestBuildId !== BUILD_ID);
  if (!updateAvailable) return null;

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex justify-center p-4">
      <div className="pointer-events-auto flex items-center gap-3 rounded-full border border-border bg-popover/95 px-4 py-2 text-sm shadow-lg backdrop-blur">
        <RefreshCwIcon className="size-4 text-muted-foreground" />
        <span className="text-popover-foreground">A new version of Iris is available.</span>
        <Button size="sm" onClick={() => window.location.reload()}>
          Reload
        </Button>
      </div>
    </div>
  );
}
