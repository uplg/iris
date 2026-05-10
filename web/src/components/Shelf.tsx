import type { ReactNode } from "react";
import { Link } from "react-router";
import { ChevronRight } from "lucide-react";

import { cn } from "@/lib/utils";

export type ShelfProps = {
  /** Section heading shown above the cards. */
  title: string;
  /** Optional "see all" link in the heading. */
  href?: string;
  /** Anything to put in the top-right of the heading row (a count badge,
   *  a filter chip…). Mutually OK with `href` — both render side by side. */
  action?: ReactNode;
  /** When the underlying query fails or returns an empty list, render
   *  this instead of an empty horizontal scroll. Keeps the home page
   *  honest about why a shelf is empty (vs. silently disappearing). */
  emptyState?: ReactNode;
  children: ReactNode;
  /** Hide the entire section when there's nothing to show AND no
   *  `emptyState` was provided. Default `true` — pages can opt out for
   *  shelves that should always be visible (e.g., onboarding nudges). */
  hideWhenEmpty?: boolean;
  /** Detected by the parent — if there's literally nothing to render,
   *  combined with `hideWhenEmpty` to elide the whole shelf. */
  isEmpty?: boolean;
  className?: string;
};

/**
 * One horizontal row of media cards. Used for every shelf on the home
 * (Continue Watching / Watchlist / New / Library). Plain horizontal
 * overflow scroll — no JS-driven carousel, the OS scrollbar / trackpad
 * gesture handles it cleanly.
 */
export function Shelf({
  title,
  href,
  action,
  emptyState,
  children,
  hideWhenEmpty = true,
  isEmpty = false,
  className,
}: ShelfProps) {
  if (isEmpty && hideWhenEmpty && !emptyState) return null;

  return (
    <section className={cn("grid gap-3", className)}>
      <div className="flex items-end justify-between gap-3">
        <div className="flex items-center gap-2">
          {href ? (
            <Link
              to={href}
              className="group flex items-center gap-1 text-base font-semibold tracking-tight text-foreground hover:text-primary"
            >
              {title}
              <ChevronRight className="size-4 opacity-0 transition group-hover:opacity-100" />
            </Link>
          ) : (
            <h2 className="text-base font-semibold tracking-tight text-foreground">{title}</h2>
          )}
        </div>
        {action && <div className="text-xs text-muted-foreground">{action}</div>}
      </div>

      {isEmpty ? (
        <div className="rounded-md border border-dashed border-border bg-card/30 p-6 text-sm text-muted-foreground">
          {emptyState}
        </div>
      ) : (
        // Negative margin + padding trick so focus rings on the first/last
        // card don't get clipped by overflow-x-auto. Hide the scrollbar on
        // wider viewports for a cleaner look — gesture scroll still works.
        // `snap-x snap-proximity` lets trackpad / arrow-key scrolling
        // settle near whole cards without forcing it (mandatory snap
        // fights the user too aggressively on long shelves).
        <div className="-mx-1 snap-x snap-proximity overflow-x-auto scroll-smooth px-1 pb-2 [&>*>*]:snap-start [scrollbar-width:thin]">
          <div className="flex gap-4">{children}</div>
        </div>
      )}
    </section>
  );
}
