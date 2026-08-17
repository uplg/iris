import { useEffect } from "react";
import { Outlet, useRouter } from "@tanstack/react-router";
import { useAuth } from "@/lib/auth";

export function RequireAuth({ adminOnly = false }: { adminOnly?: boolean }) {
  const auth = useAuth();
  const router = useRouter();

  const bounce =
    auth.status === "anonymous"
      ? "login"
      : auth.status === "authenticated" && adminOnly && !auth.user.is_admin
        ? "home"
        : null;

  // Imperative redirect from a passive effect — NOT a rendered `<Navigate>`.
  // Navigate's layout effect compares its props by reference, so an inline
  // props object re-navigates on every render; while the lazy /login chunk
  // is pending this layout stays mounted, and navigate → commit → render →
  // navigate livelocks the main thread (the tab hard-freezes). Keying the
  // effect on the string `bounce` fires exactly once per transition.
  useEffect(() => {
    if (bounce === "login") {
      // Remember where to return after sign-in (pathname only — matches the
      // previous `state.from.pathname` behaviour). Skip when already mid-
      // bounce so a late re-run can't clobber the redirect with "/login".
      const { pathname } = router.state.location;
      if (pathname !== "/login") {
        void router.navigate({ to: "/login", search: { redirect: pathname }, replace: true });
      }
    } else if (bounce === "home") {
      void router.navigate({ to: "/", replace: true });
    }
  }, [bounce, router]);

  if (auth.status === "loading") {
    return (
      <div className="flex min-h-screen items-center justify-center text-muted-foreground">
        {auth.retrying ? "connection is unstable — retrying…" : "loading…"}
      </div>
    );
  }
  if (bounce) return null;
  return <Outlet />;
}
