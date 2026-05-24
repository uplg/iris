import { Link } from "react-router";

/**
 * The Iris wordmark — used as the logo in the header and as a focal point
 * on the auth pages. The gradient tracks the active accent (`brand-text`
 * utility) so it stays in sync with the user's chosen colour.
 */
export function Brand({
  size = "md",
  asLink = true,
}: {
  size?: "sm" | "md" | "lg";
  asLink?: boolean;
}) {
  const fontSize = size === "lg" ? 40 : size === "sm" ? 16 : 22;

  const inner = (
    <span className="brand-text inline-flex items-baseline gap-1" style={{ fontSize }}>
      Iris
      <span
        className="font-normal text-fg-dim"
        style={{ fontSize: fontSize * 0.55, WebkitTextFillColor: "currentcolor" }}
      >
        /
      </span>
    </span>
  );

  if (!asLink) return inner;
  return (
    <Link to="/" className="focus-ring inline-flex rounded-md">
      {inner}
    </Link>
  );
}
