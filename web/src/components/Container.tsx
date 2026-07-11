import { cn } from "@/lib/utils";

/**
 * Centered content column. The app shell's <main> is full-bleed so hero
 * backdrops can span the viewport; contained sections wrap their content in
 * a <Container> to get the 1280px max width + responsive gutters that match
 * the redesign.
 */
export function Container({
  children,
  className,
  narrow = false,
  wide = false,
}: {
  children: React.ReactNode;
  className?: string;
  /** 960px column — used for reading-width pages like Account. */
  narrow?: boolean;
  /** Full viewport width (gutters kept) — the watch page's theater
   *  mode. Wins over `narrow`. */
  wide?: boolean;
}) {
  return (
    <div
      className={cn(
        "mx-auto w-full px-4 sm:px-6 lg:px-8",
        wide ? "max-w-none" : narrow ? "max-w-5xl" : "max-w-[1280px]",
        className,
      )}
    >
      {children}
    </div>
  );
}
