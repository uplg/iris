import { useEffect, useState } from "react";
import { Link } from "react-router";
import { ChevronRight, Search as SearchIcon, Tv } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const STORAGE_KEY = "iris.onboarded";

/**
 * One-shot 2-step welcome wizard. Shown on first mount per browser
 * (the dismissal is local-storage-pinned, so a logout/login cycle
 * doesn't re-show it on the same device, but switching devices does
 * so each new install gets the orientation).
 *
 * Why localStorage and not a server-side `users.onboarded_at`: keeps
 * the change scoped to the frontend and avoids another migration. The
 * tradeoff is signing in on a fresh browser re-shows the wizard —
 * we consider that desirable (re-orient the user on the new device).
 */
export function OnboardingWizard() {
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<0 | 1>(0);

  useEffect(() => {
    if (typeof window === "undefined") return;
    if (!window.localStorage.getItem(STORAGE_KEY)) {
      setOpen(true);
    }
  }, []);

  const dismiss = () => {
    try {
      window.localStorage.setItem(STORAGE_KEY, new Date().toISOString());
    } catch {
      // localStorage can throw in private mode — non-fatal, the wizard
      // simply re-shows next session.
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
          <DialogTitle className="flex items-center gap-2">
            Welcome to Iris
            <span className="text-xs font-normal text-muted-foreground">
              {step + 1}/2
            </span>
          </DialogTitle>
          <DialogDescription>
            {step === 0
              ? "A quick step to get you oriented."
              : "You're ready — here's where to start."}
          </DialogDescription>
        </DialogHeader>

        {step === 0 ? (
          <Step
            icon={<Tv className="size-7" />}
            title="Got an Android TV?"
            body="Iris has a TV app. Pair it from your account settings to watch from the couch."
            actions={
              <>
                <Button asChild>
                  <Link to="/account" onClick={() => setStep(1)}>
                    Pair my TV
                    <ChevronRight className="size-4" />
                  </Link>
                </Button>
                <Button variant="ghost" onClick={() => setStep(1)}>
                  Not now
                </Button>
              </>
            }
          />
        ) : (
          <Step
            icon={<SearchIcon className="size-7" />}
            title="Find something to watch"
            body="Search surfaces fresh indexer hits and lets you tap one to grab it instantly."
            actions={
              <>
                <Button asChild>
                  <Link to="/search" onClick={dismiss}>
                    Open search
                    <ChevronRight className="size-4" />
                  </Link>
                </Button>
                <Button variant="ghost" onClick={dismiss}>
                  Explore later
                </Button>
              </>
            }
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Step({
  icon,
  title,
  body,
  actions,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
  actions: React.ReactNode;
}) {
  return (
    <div className="grid gap-4 py-2">
      <div className={cn("flex items-center gap-3 text-primary")}>{icon}</div>
      <div className="grid gap-1">
        <h3 className="text-base font-semibold tracking-tight">{title}</h3>
        <p className="text-sm text-muted-foreground">{body}</p>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">{actions}</div>
    </div>
  );
}
