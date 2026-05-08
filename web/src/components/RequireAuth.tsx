import { Navigate, Outlet, useLocation } from "react-router";
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
    return <Navigate to="/login" replace state={{ from: location }} />;
  }
  if (adminOnly && !auth.user.is_admin) {
    return <Navigate to="/" replace />;
  }
  return <Outlet />;
}
