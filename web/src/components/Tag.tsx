import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type TagVariant = "plain" | "accent" | "success" | "warn";

const VARIANTS: Record<TagVariant, string> = {
  plain: "bg-elev-2 text-muted-foreground border-border",
  accent: "bg-brand-soft text-primary border-transparent",
  success: "bg-success/15 text-success border-transparent",
  warn: "bg-warn/15 text-warn border-transparent",
};

/**
 * Small status pill matching the redesign's `Badge` look (the design tool's
 * accent / success / plain chips). Distinct from the shadcn `ui/badge`
 * primitive so we keep that file shadcn-managed; this is the app-level chip
 * used for "FL", "In library", "Season pack", "Now playing", etc.
 */
export function Tag({
  variant = "plain",
  upper = false,
  className,
  children,
}: {
  variant?: TagVariant;
  upper?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] items-center gap-1 rounded-full border px-2 text-[11px] font-medium",
        upper && "text-[10px] uppercase tracking-[0.08em]",
        VARIANTS[variant],
        className,
      )}
    >
      {children}
    </span>
  );
}
