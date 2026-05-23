import { useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

const STORAGE_KEY = "iris.firefox-warning-ack";

/**
 * Firefox-only "browser happy" warning. Iris's playback pipeline (Tier
 * B Mediabunny remux + MSE, Tier F shaka HLS) is significantly more
 * stable on Chromium-family browsers — Firefox's WebCodecs Audio
 * encoder lacks AAC-in-MP4 (we fall back to Opus stereo, but seek is
 * still flaky), and its VideoToolbox bridge on macOS chokes on 4K
 * HEVC HDR. Until we ship a server-side H.264 transcode fallback,
 * the honest advice is "use Chrome".
 *
 * Acknowledgement is local-storage-pinned per browser so we don't nag
 * after the user has been warned once.
 */
export function FirefoxWarning() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined") return;
    if (!isFirefox()) return;
    try {
      if (window.localStorage.getItem(STORAGE_KEY)) return;
    } catch {
      // localStorage unavailable (private mode) — still show, just
      // can't pin the ack. Re-shows next session.
    }
    setOpen(true);
  }, []);

  const dismiss = () => {
    try {
      window.localStorage.setItem(STORAGE_KEY, new Date().toISOString());
    } catch {
      /* non-fatal */
    }
    setOpen(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) dismiss();
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-amber-500">
            <AlertTriangle className="size-5" />
            Browser compatibility notice
          </DialogTitle>
          <DialogDescription>
            For the smoothest playback experience, please use{" "}
            <span className="text-foreground">Chrome</span> (or any Chromium-based browser).
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 py-2 text-sm text-muted-foreground">
          <p>
            Firefox works for most files, but some playback paths — in particular 4K HEVC, Dolby
            Vision, and seeking on long files — are flaky on it. We're tracking proper Firefox
            support; in the meantime Chrome gives a far more reliable experience.
          </p>
        </div>

        <div className="flex justify-end gap-2">
          <Button onClick={dismiss}>Got it</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function isFirefox(): boolean {
  if (typeof navigator === "undefined") return false;
  // Match Firefox-proper + Firefox-derived (LibreWolf, Waterfox, …).
  // We deliberately don't match Seamonkey / IceCat / etc. since the
  // playback engines differ; if anyone reports issues there we'll
  // widen the regex.
  return /Firefox\/\d+/.test(navigator.userAgent);
}
