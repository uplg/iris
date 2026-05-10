import { Link, NavLink, Outlet, useNavigate } from "react-router";
import { Home as HomeIcon, Library as LibraryIcon, LogOut, Search as SearchIcon, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Brand } from "@/components/Brand";
import { OnboardingWizard } from "@/components/OnboardingWizard";
import { ThemeToggle } from "@/components/ThemeToggle";
import { useAuth } from "@/lib/auth";

export function AppShell() {
  const auth = useAuth();
  const navigate = useNavigate();

  if (auth.status !== "authenticated") return null;

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b border-border">
        <div className="mx-auto flex max-w-6xl items-center justify-between gap-6 px-6 py-4">
          <Brand />
          <nav className="flex items-center gap-1 text-sm">
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
            <Link
              to="/account"
              className="ml-2 hidden text-xs text-muted-foreground hover:text-foreground sm:inline"
              title="Account settings"
            >
              {auth.user.email}
            </Link>
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
      <main className="mx-auto max-w-6xl px-6 py-10">
        <Outlet />
      </main>
      <OnboardingWizard />
    </div>
  );
}

function NavItem({
  to,
  end,
  icon,
  children,
}: {
  to: string;
  end?: boolean;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) =>
        `inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 transition ${
          isActive
            ? "bg-muted text-foreground"
            : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
        }`
      }
    >
      {icon}
      <span>{children}</span>
    </NavLink>
  );
}
