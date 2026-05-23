import { NavLink, Outlet, useNavigate } from "react-router";
import {
  Home as HomeIcon,
  Library as LibraryIcon,
  LogOut,
  Search as SearchIcon,
  ShieldCheck,
  User as UserIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Brand } from "@/components/Brand";
import { FirefoxWarning } from "@/components/FirefoxWarning";
import { ThemeToggle } from "@/components/ThemeToggle";
import { useAuth } from "@/lib/auth";

export function AppShell() {
  const auth = useAuth();
  const navigate = useNavigate();

  if (auth.status !== "authenticated") return null;

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b border-border">
        <div className="mx-auto flex max-w-6xl items-center justify-between gap-2 px-3 py-3 sm:gap-6 sm:px-6 sm:py-4">
          <Brand />
          <nav className="flex items-center gap-0.5 text-sm sm:gap-1">
            <NavItem to="/" end icon={<HomeIcon className="size-4" />}>
              Home
            </NavItem>
            <NavItem to="/search" icon={<SearchIcon className="size-4" />}>
              Search
            </NavItem>
            <NavItem to="/library" icon={<LibraryIcon className="size-4" />}>
              Library
            </NavItem>
            {auth.user.is_admin && (
              <NavItem to="/admin" icon={<ShieldCheck className="size-4" />}>
                Admin
              </NavItem>
            )}
            <NavItem to="/account" icon={<UserIcon className="size-4" />} label={auth.user.email}>
              <span className="hidden max-w-[12ch] truncate lg:inline">{auth.user.email}</span>
            </NavItem>
            <ThemeToggle />
            <Button
              variant="ghost"
              size="sm"
              onClick={async () => {
                await auth.logout();
                navigate("/login", { replace: true });
              }}
              title="Sign out"
            >
              <LogOut className="size-4" />
              <span className="sr-only sm:not-sr-only sm:ml-1">Sign out</span>
            </Button>
          </nav>
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-3 py-6 sm:px-6 sm:py-10">
        <Outlet />
      </main>
      <FirefoxWarning />
    </div>
  );
}

function NavItem({
  to,
  end,
  icon,
  children,
  label,
}: {
  to: string;
  end?: boolean;
  icon: React.ReactNode;
  children: React.ReactNode;
  /** Accessible name + tooltip. Defaults to `children` when it's a
   *  plain string. Required when the label is hidden on mobile so the
   *  icon-only control still announces itself to screen readers. */
  label?: string;
}) {
  const accessibleName = label ?? (typeof children === "string" ? children : undefined);
  return (
    <NavLink
      to={to}
      end={end}
      title={accessibleName}
      aria-label={accessibleName}
      className={({ isActive }) =>
        `inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 transition sm:px-2.5 ${
          isActive
            ? "bg-muted text-foreground"
            : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
        }`
      }
    >
      {icon}
      {/* Labels collapse to icons-only below `sm` so the bar never
          overflows a ~390px phone. The account item passes its own
          nested span (email shown only from `lg`). */}
      {typeof children === "string" ? (
        <span className="hidden sm:inline">{children}</span>
      ) : (
        children
      )}
    </NavLink>
  );
}
