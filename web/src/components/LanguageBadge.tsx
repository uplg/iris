import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

/**
 * Tiny FR / EN / MULTi pill rendered next to search results and
 * available-episode rows. Drops to `null` for unknown / missing
 * tags so we don't fill the UI with placeholder noise — every other
 * indexer release is "MULTi" anyway, the interesting signal is
 * "this one is English-only" for the household's anglophone users.
 *
 * Stable colour mapping (don't reshuffle — users learn the badges):
 *   - FR   : blue   (matches the French tricolore left-bar tone)
 *   - EN   : amber  (warm, contrasts with FR's blue)
 *   - MULTi: emerald (the "satisfies both" tag)
 */
export function LanguageBadge({
  language,
  className,
}: {
  language: string | null | undefined;
  className?: string;
}) {
  if (!language || language === "unknown") return null;
  const lower = language.toLowerCase();
  const { label, classes } =
    lower === "french"
      ? { label: "FR", classes: "bg-sky-500/90 text-white" }
      : lower === "english"
        ? { label: "EN", classes: "bg-amber-500/90 text-white" }
        : lower === "multi"
          ? { label: "MULTi", classes: "bg-emerald-500/90 text-white" }
          : { label: lower.toUpperCase(), classes: "bg-muted text-foreground" };
  return (
    <Badge className={cn("text-[10px] uppercase shadow-md", classes, className)}>{label}</Badge>
  );
}
