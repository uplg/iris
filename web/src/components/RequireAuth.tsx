import { Navigate, Outlet, useLocation } from "@tanstack/react-router";
import { useAuth } from "@/lib/auth";

export function RequireAuth({ adminOnly = false }: { adminOnly?: boolean }) {
  const auth = useAuth();
  const location = useLocation();

  if (auth.status === "loading") {
    return (
      <div className="flex min-h-screen items-center justify-center text-muted-foreground">
        loading…
      </div>
    );
  }
  if (auth.status === "anonymous") {
    // Bounce to login, remembering where to return after sign-in (pathname
    // only — matches the previous `state.from.pathname` behaviour).
    return <Navigate to="/login" search={{ redirect: location.pathname }} replace />;
  }
  if (adminOnly && !auth.user.is_admin) {
    return <Navigate to="/" replace />;
  }
  return <Outlet />;
}
