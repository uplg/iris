import { type ReactNode, useRef } from "react";
import { Link, type LinkProps } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type ShelfProps = {
  /** Small uppercase label above the title (e.g. "For you", "Fresh"). */
  eyebrow?: string;
  /** Section heading shown above the cards. */
  title: string;
  /** Optional "see all" link in the heading. Build it with
   *  `linkOptions({ to })` for type-safety. */
  link?: LinkProps;
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
 * overflow scroll with optional arrow controls on the desktop — the OS
 * scrollbar / trackpad gesture still handles it on touch.
 */
export function Shelf({
  eyebrow,
  title,
  link,
  action,
  emptyState,
  children,
  hideWhenEmpty = true,
  isEmpty = false,
  className,
}: ShelfProps) {
  const scrollerRef = useRef<HTMLDivElement>(null);

  if (isEmpty && hideWhenEmpty && !emptyState) return null;

  const scrollBy = (dir: number) => {
    const el = scrollerRef.current;
    if (!el) return;
    el.scrollBy({ left: dir * Math.min(640, el.clientWidth * 0.85), behavior: "smooth" });
  };

  return (
    <section className={cn("grid gap-3.5", className)}>
      <div className="flex items-end justify-between gap-4">
        <div className="grid gap-1">
          {eyebrow && <span className="eyebrow">{eyebrow}</span>}
          {link ? (
            <Link
              {...link}
              className="group inline-flex items-center gap-1.5 font-display text-[22px] tracking-tight text-foreground transition-colors hover:text-primary"
            >
              {title}
              <ChevronRight className="size-[18px] opacity-40 transition group-hover:translate-x-0.5 group-hover:opacity-100" />
            </Link>
          ) : (
            <h2 className="font-display text-[22px] tracking-tight text-foreground">{title}</h2>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          {action && <div className="text-xs text-muted-foreground">{action}</div>}
          {!isEmpty && (
            <div className="hidden gap-1 md:flex">
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Scroll left"
                onClick={() => scrollBy(-1)}
              >
                <ChevronLeft className="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Scroll right"
                onClick={() => scrollBy(1)}
              >
                <ChevronRight className="size-4" />
              </Button>
            </div>
          )}
        </div>
      </div>

      {isEmpty ? (
        <div className="rounded-xl border border-dashed border-border-strong bg-surface p-7 text-sm text-fg-dim">
          {emptyState}
        </div>
      ) : (
        // Negative margin + padding trick so focus rings on the first/last
        // card don't get clipped by overflow-x-auto. Hide the scrollbar for a
        // cleaner look — gesture scroll still works. `snap-x snap-proximity`
        // lets trackpad / arrow-key scrolling settle near whole cards without
        // forcing it (mandatory snap fights the user on long shelves).
        <div
          ref={scrollerRef}
          className="no-scrollbar -mx-1 snap-x snap-proximity overflow-x-auto scroll-smooth px-1 pb-2 [&>*>*]:snap-start"
        >
          <div className="flex gap-4">{children}</div>
        </div>
      )}
    </section>
  );
}
