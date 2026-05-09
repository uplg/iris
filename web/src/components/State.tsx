import type { ReactNode } from "react";
import { AlertTriangle, Inbox, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * Empty state — the page / shelf has no data and the user needs a
 * gentle nudge toward what to do next. Distinct from an error: nothing
 * went wrong, there's just nothing here yet.
 *
 * Use in preference to `<p className="text-muted-foreground">Nothing
 * here</p>` so the visual treatment is consistent across the app.
 */
export function EmptyState({
  icon = <Inbox className="size-7" />,
  title,
  body,
  action,
  className,
}: {
  icon?: ReactNode;
  title: string;
  body?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border bg-card/30 px-6 py-10 text-center",
        className,
      )}
    >
      <div className="text-muted-foreground/60">{icon}</div>
      <div className="grid gap-1">
        <p className="text-sm font-medium text-foreground">{title}</p>
        {body && <p className="text-xs text-muted-foreground">{body}</p>}
      </div>
      {action}
    </div>
  );
}

/**
 * Error state — something went wrong, surface what + offer retry. The
 * `error` prop accepts unknown so callers can pass a TanStack Query
 * `error` directly without unwrapping.
 */
export function ErrorState({
  title = "Quelque chose a cassé",
  error,
  onRetry,
  className,
}: {
  title?: string;
  error: unknown;
  onRetry?: () => void;
  className?: string;
}) {
  const msg =
    error instanceof Error ? error.message : typeof error === "string" ? error : "Erreur inconnue";
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-6 py-8 text-center",
        className,
      )}
    >
      <AlertTriangle className="size-7 text-destructive" />
      <div className="grid gap-1">
        <p className="text-sm font-medium text-destructive">{title}</p>
        <p className="text-xs text-muted-foreground">{msg}</p>
      </div>
      {onRetry && (
        <Button variant="outline" size="sm" onClick={onRetry}>
          Réessayer
        </Button>
      )}
    </div>
  );
}

/**
 * Loading state for full-page contexts. Inline spinners stay inline —
 * use this when nothing else is on screen yet.
 */
export function LoadingState({
  label = "Chargement…",
  className,
}: {
  label?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex items-center justify-center gap-2 px-6 py-10 text-sm text-muted-foreground",
        className,
      )}
    >
      <Loader2 className="size-4 animate-spin" />
      {label}
    </div>
  );
}

/**
 * Pulse-animated card placeholder for shelves while data is loading.
 * Better than a single spinner because it preserves the layout (no
 * jump-shift when cards arrive).
 */
export function SkeletonCard({
  count = 6,
  className,
}: {
  count?: number;
  className?: string;
}) {
  return (
    <div className={cn("flex gap-4", className)}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="w-40 shrink-0">
          <div className="aspect-[2/3] animate-pulse rounded-lg bg-muted/40" />
          <div className="mt-2 h-3 animate-pulse rounded bg-muted/40" />
          <div className="mt-1 h-2 w-2/3 animate-pulse rounded bg-muted/30" />
        </div>
      ))}
    </div>
  );
}
