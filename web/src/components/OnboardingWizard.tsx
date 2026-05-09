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
            Bienvenue sur Iris
            <span className="text-xs font-normal text-muted-foreground">
              {step + 1}/2
            </span>
          </DialogTitle>
          <DialogDescription>
            {step === 0
              ? "Une étape rapide pour t'orienter."
              : "Tu es prêt — voici par où commencer."}
          </DialogDescription>
        </DialogHeader>

        {step === 0 ? (
          <Step
            icon={<Tv className="size-7" />}
            title="Tu as une Android TV ?"
            body="Iris a une appli TV. Pair-la depuis tes paramètres pour regarder sur le canapé."
            actions={
              <>
                <Button asChild>
                  <Link to="/account" onClick={() => setStep(1)}>
                    Pairer ma TV
                    <ChevronRight className="size-4" />
                  </Link>
                </Button>
                <Button variant="ghost" onClick={() => setStep(1)}>
                  Pas pour l'instant
                </Button>
              </>
            }
          />
        ) : (
          <Step
            icon={<SearchIcon className="size-7" />}
            title="Trouve ce que tu veux regarder"
            body="La recherche surface les sorties TMDB en temps réel et te laisse cliquer pour télécharger directement."
            actions={
              <>
                <Button asChild>
                  <Link to="/search" onClick={dismiss}>
                    Lancer une recherche
                    <ChevronRight className="size-4" />
                  </Link>
                </Button>
                <Button variant="ghost" onClick={dismiss}>
                  Explorer plus tard
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
