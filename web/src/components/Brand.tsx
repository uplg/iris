import { Link } from "react-router";

/**
 * The Iris wordmark — used as the logo in the header and as a focal point
 * on the auth pages.
 */
export function Brand({
  size = "md",
  asLink = true,
}: {
  size?: "sm" | "md" | "lg";
  asLink?: boolean;
}) {
  const sizeClass = size === "lg" ? "text-4xl" : size === "sm" ? "text-base" : "text-lg";

  const inner = (
    <span
      className={`${sizeClass} font-semibold tracking-tight inline-flex items-baseline gap-1.5`}
    >
      <span className="bg-linear-to-r from-fuchsia-400 via-violet-400 to-sky-400 bg-clip-text text-transparent">
        Iris
      </span>
      <span className="font-light text-muted-foreground">/</span>
    </span>
  );

  if (!asLink) return inner;
  return <Link to="/">{inner}</Link>;
}
