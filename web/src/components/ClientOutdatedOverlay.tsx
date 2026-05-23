import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { CLIENT_OUTDATED_EVENT, IRIS_WEB_VERSION } from "@/lib/api";

/**
 * Listens for [CLIENT_OUTDATED_EVENT] (dispatched by `api.ts` on any
 * `426 Upgrade Required` response) and renders a full-screen
 * lock-out telling the user to refresh.
 *
 * The remediation on Web is `window.location.reload()` — Iris's web
 * bundle is served by the same backend that just returned the 426,
 * so a fresh request will pull the new bundle once the backend has
 * been redeployed. The button is the primary action; we also detect
 * Cmd/Ctrl-R but the OS handles those natively.
 *
 * Once flipped, the overlay does NOT clear by itself — the deployed
 * bundle is stale by definition. The user must reload.
 */
export function ClientOutdatedOverlay() {
  const [outdated, setOutdated] = useState(false);

  useEffect(() => {
    const onOutdated = () => setOutdated(true);
    window.addEventListener(CLIENT_OUTDATED_EVENT, onOutdated);
    return () => window.removeEventListener(CLIENT_OUTDATED_EVENT, onOutdated);
  }, []);

  if (!outdated) return null;

  return (
    <div
      role="alertdialog"
      aria-labelledby="iris-outdated-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 p-8 backdrop-blur"
    >
      <div className="max-w-md space-y-4 text-center">
        <h1 id="iris-outdated-title" className="text-2xl font-semibold">
          Update Iris
        </h1>
        <p className="text-muted-foreground">
          The Iris server requires a newer client. Reload the page to pull the latest bundle.
        </p>
        <p className="text-xs text-muted-foreground">
          Cached version: <code className="font-mono">{IRIS_WEB_VERSION}</code>
        </p>
        <Button onClick={() => window.location.reload()}>Reload</Button>
      </div>
    </div>
  );
}
