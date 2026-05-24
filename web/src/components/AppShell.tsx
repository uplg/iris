import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router";
import {
  Home as HomeIcon,
  Library as LibraryIcon,
  Settings as SettingsIcon,
  Search as SearchIcon,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Brand } from "@/components/Brand";
import { FirefoxWarning } from "@/components/FirefoxWarning";
import { TweaksDrawer } from "@/components/TweaksDrawer";
import { useAuth } from "@/lib/auth";
import { cn } from "@/lib/utils";

type NavEntry = {
  to: string;
  label: string;
  icon: typeof HomeIcon;
  end?: boolean;
  adminOnly?: boolean;
};

export function AppShell() {
  const auth = useAuth();
  const { pathname } = useLocation();
  const [scrolled, setScrolled] = useState(false);
  const [tweaksOpen, setTweaksOpen] = useState(false);

  // Home and the collection detail render a full-bleed backdrop hero that
  // intentionally tucks under the transparent sticky header — they must NOT
  // get top padding (it would leave a strip of bg above the artwork). Every
  // other page starts with a heading, so give it breathing room below the bar.
  const fullBleedHero = pathname === "/" || pathname.startsWith("/collection/");

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 4);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  if (auth.status !== "authenticated") return null;

  const nav: NavEntry[] = [
    { to: "/", label: "Home", icon: HomeIcon, end: true },
    { to: "/search", label: "Search", icon: SearchIcon },
    { to: "/library", label: "Library", icon: LibraryIcon },
    ...(auth.user.is_admin
      ? [{ to: "/admin", label: "Admin", icon: ShieldCheck } satisfies NavEntry]
      : []),
  ];

  const avatarLetter = (auth.user.display_name || auth.user.email).charAt(0).toUpperCase();

  return (
    <div className="flex min-h-svh flex-col bg-background text-foreground">
      <header
        className={cn(
          "sticky top-0 z-40 backdrop-blur-xl transition-colors duration-200",
          scrolled ? "border-b border-border bg-surface-blur" : "border-b border-transparent",
        )}
        style={{ backdropFilter: "blur(16px) saturate(140%)" }}
      >
        <div
          className="mx-auto flex max-w-[1280px] items-center gap-4 px-4 sm:gap-6 sm:px-6 lg:px-8"
          style={{ height: "var(--header-h)" }}
        >
          <Brand />
          <nav className="ml-2 hidden items-center gap-0.5 md:flex">
            {nav.map((item) => (
              <NavItem key={item.to} item={item} />
            ))}
          </nav>
          <div className="flex-1" />
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Display settings"
              onClick={() => setTweaksOpen((v) => !v)}
            >
              <SettingsIcon className="size-4" />
            </Button>
            <NavLink
              to="/account"
              aria-label="Account"
              className="focus-ring inline-flex h-9 items-center gap-2 rounded-full border border-border bg-elev py-0 pr-1 pl-2.5"
            >
              <span className="hidden max-w-[16ch] truncate text-[13px] text-muted-foreground lg:inline">
                {auth.user.email}
              </span>
              <span
                className="grid size-7 place-items-center rounded-full font-display text-xs font-semibold text-primary-foreground"
                style={{ background: "linear-gradient(135deg, var(--brand-3), var(--brand))" }}
              >
                {avatarLetter}
              </span>
            </NavLink>
          </div>
        </div>
      </header>

      <main className={cn("flex-1 pb-24 md:pb-12", !fullBleedHero && "pt-8 sm:pt-10")}>
        <Outlet />
      </main>

      <BottomBar nav={nav} />

      <TweaksDrawer open={tweaksOpen} onClose={() => setTweaksOpen(false)} />
      <FirefoxWarning />
    </div>
  );
}

function NavItem({ item }: { item: NavEntry }) {
  const Icon = item.icon;
  return (
    <NavLink
      to={item.to}
      end={item.end}
      className={({ isActive }) =>
        cn(
          "relative inline-flex items-center gap-2 rounded-[10px] px-3 py-2 text-sm font-medium transition-colors",
          isActive
            ? "bg-elev-2 text-foreground"
            : "text-muted-foreground hover:bg-accent hover:text-foreground",
        )
      }
    >
      {({ isActive }) => (
        <>
          <Icon className="size-4" />
          <span>{item.label}</span>
          {isActive && (
            <span className="absolute inset-x-4 -bottom-px h-0.5 rounded-full bg-primary" />
          )}
        </>
      )}
    </NavLink>
  );
}

function BottomBar({ nav }: { nav: NavEntry[] }) {
  return (
    <nav
      className="fixed inset-x-0 bottom-0 z-30 border-t border-border bg-surface-blur md:hidden"
      style={{
        backdropFilter: "blur(20px) saturate(140%)",
        paddingBottom: "calc(8px + env(safe-area-inset-bottom))",
        paddingTop: 8,
      }}
    >
      <div
        className="mx-auto grid max-w-[520px] gap-1 px-2"
        style={{ gridTemplateColumns: `repeat(${nav.length}, 1fr)` }}
      >
        {nav.map((item) => {
          const Icon = item.icon;
          return (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) =>
                cn(
                  "grid place-items-center gap-0.5 rounded-xl px-1 py-1.5 text-[11px] font-medium leading-none",
                  isActive ? "bg-accent text-foreground" : "text-fg-dim",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <Icon className={cn("size-5", isActive && "text-primary")} />
                  <span className="mt-0.5">{item.label}</span>
                </>
              )}
            </NavLink>
          );
        })}
      </div>
    </nav>
  );
}
