import type { ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { Brand } from "@/components/Brand";
import { Button } from "@/components/ui/button";

/**
 * Branded "nothing here" state. Replaces the bare-bones fallbacks (the
 * router's default Not Found, WatchPage's raw error strings) with the
 * Iris wordmark, a readable explanation, and a way out. `actions`
 * overrides the default "Back to home" button when a page has a more
 * useful escape hatch (e.g. "Open library").
 */
export function NotFoundState({
  eyebrow = "404",
  title,
  description,
  actions,
}: {
  /** Small uppercase label above the title; `null` hides it. */
  eyebrow?: string | null;
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="flex min-h-[60svh] flex-col items-center justify-center gap-7 px-6 py-16 text-center">
      <Brand size="lg" asLink={false} />
      <div className="grid gap-2.5">
        {eyebrow && (
          <p className="text-xs font-semibold tracking-[0.25em] text-muted-foreground uppercase">
            {eyebrow}
          </p>
        )}
        <h1 className="font-display text-2xl font-semibold text-balance sm:text-3xl">{title}</h1>
        {description && (
          <p className="mx-auto max-w-md text-sm text-pretty text-muted-foreground">
            {description}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-center gap-3">
        {actions ?? <Button render={<Link to="/" />}>Back to home</Button>}
      </div>
    </div>
  );
}

/** Full-page 404 wired as the router's `defaultNotFoundComponent`. It can
 *  render outside the authed shell (unknown URLs are reachable anonymous),
 *  so it paints its own background instead of relying on AppShell. */
export function NotFoundPage() {
  return (
    <div className="min-h-svh bg-background text-foreground">
      <NotFoundState
        title="This page doesn't exist"
        description="The link may be broken, or the page may have moved."
      />
    </div>
  );
}
