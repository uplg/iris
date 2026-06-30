import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import { AppShell } from "@/components/AppShell";
import { ClientOutdatedOverlay } from "@/components/ClientOutdatedOverlay";
import { RequireAuth } from "@/components/RequireAuth";
import { UpdateBanner } from "@/components/UpdateBanner";

// Page components are code-split: each `import()` below is a literal
// specifier (Vite needs that to emit a separate chunk), so a page's code —
// incl. the heavy player graph behind /watch — only loads on navigation,
// and is prefetched on link hover/focus via `defaultPreload`. The layout
// components (RequireAuth, AppShell) stay eager: they render the shell
// immediately. `lazyRouteComponent`'s 2nd arg is the named export (pages
// export `export function XPage`, not default).

/** Search-param contracts. `validateSearch` is the only sanctioned entry
 *  point for query state, so these shapes are what `useSearch()` returns
 *  and what `navigate({ search })` is type-checked against. */
export type LibrarySort = "alpha" | "recent" | "size";

const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);

/** Root: renders the active route plus the two app-global overlays that
 *  used to sit beside `<Routes>`. */
function RootLayout() {
  return (
    <>
      <Outlet />
      <ClientOutdatedOverlay />
      <UpdateBanner />
    </>
  );
}

/** Admin-only guard layer — reuses [`RequireAuth`] with `adminOnly`. The
 *  outer auth layout already bounced anonymous users, so this only adds
 *  the `is_admin` check before the `/admin` child renders. */
function AdminGuard() {
  return <RequireAuth adminOnly />;
}

/** Shown while a lazily-loaded route chunk is in flight. `defaultPendingMs`
 *  (1s) gates it, so a fast chunk fetch never flickers a spinner. */
function RoutePending() {
  return (
    <div className="flex min-h-[50svh] items-center justify-center text-muted-foreground">
      loading…
    </div>
  );
}

const rootRoute = createRootRoute({ component: RootLayout });

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  // Where to land after a successful sign-in (set by RequireAuth when it
  // bounces an anonymous user). Pathname only — mirrors the old
  // `state.from.pathname` behaviour.
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => ({
    redirect: str(search.redirect),
  }),
  component: lazyRouteComponent(() => import("@/pages/LoginPage"), "LoginPage"),
});

const registerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/register",
  // `?token=…` pre-fills the invitation field from a shared invite link.
  validateSearch: (search: Record<string, unknown>): { token?: string } => ({
    token: str(search.token),
  }),
  component: lazyRouteComponent(() => import("@/pages/RegisterPage"), "RegisterPage"),
});

// Pathless layout: gate every authed route behind the session check.
const authRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "auth",
  component: RequireAuth,
});

// Pathless layout: the chrome (header nav + bottom bar) around authed pages.
const shellRoute = createRoute({
  getParentRoute: () => authRoute,
  id: "shell",
  component: AppShell,
});

const indexRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/",
  component: lazyRouteComponent(() => import("@/pages/HomePage"), "HomePage"),
});

const forYouRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/for-you",
  component: lazyRouteComponent(() => import("@/pages/ForYouPage"), "ForYouPage"),
});

const moodsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/moods",
  // `mood` selects a tile's results; `kind` toggles Film/Series. Empty `mood`
  // shows the board grid.
  validateSearch: (search: Record<string, unknown>): { mood?: string; kind?: "movie" | "tv" } => ({
    mood: str(search.mood),
    kind: search.kind === "tv" ? "tv" : search.kind === "movie" ? "movie" : undefined,
  }),
  component: lazyRouteComponent(() => import("@/pages/MoodsPage"), "MoodsPage"),
});

const searchRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/search",
  validateSearch: (search: Record<string, unknown>): { q?: string } => ({
    q: str(search.q),
  }),
  component: lazyRouteComponent(() => import("@/pages/SearchPage"), "SearchPage"),
});

// Legacy `/series/:followId` → `/collection/:id` redirect. The C1 façade
// returns `collection.id` as the follow id, so it's a straight
// pass-through; ids that don't resolve 404 inside CollectionPage.
const seriesAliasRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/series/$followId",
  beforeLoad: ({ params }) => {
    throw redirect({
      to: "/collection/$id",
      params: { id: params.followId },
      replace: true,
    });
  },
});

const collectionRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/collection/$id",
  component: lazyRouteComponent(() => import("@/pages/CollectionPage"), "CollectionPage"),
});

const libraryRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/library",
  // `view` toggles collections/torrents; `kind` + `sort` are the
  // collections-view filters, persisted so a refresh keeps the choice.
  validateSearch: (
    search: Record<string, unknown>,
  ): { view?: "torrents"; kind?: "movie" | "tv"; sort?: LibrarySort } => ({
    view: search.view === "torrents" ? "torrents" : undefined,
    kind: search.kind === "movie" || search.kind === "tv" ? search.kind : undefined,
    sort:
      search.sort === "alpha" || search.sort === "recent" || search.sort === "size"
        ? search.sort
        : undefined,
  }),
  component: lazyRouteComponent(() => import("@/pages/LibraryPage"), "LibraryPage"),
});

const watchRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/watch/$infohash/$idx",
  component: lazyRouteComponent(() => import("@/pages/WatchPage"), "WatchPage"),
});

const historyRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/history",
  component: lazyRouteComponent(() => import("@/pages/HistoryPage"), "HistoryPage"),
});

const accountRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/account",
  // `?pair=ABCD-EFGH` deep-links a TV pairing code into the input.
  validateSearch: (search: Record<string, unknown>): { pair?: string } => ({
    pair: str(search.pair),
  }),
  component: lazyRouteComponent(() => import("@/pages/AccountPage"), "AccountPage"),
});

const adminGuardRoute = createRoute({
  getParentRoute: () => shellRoute,
  id: "adminGuard",
  component: AdminGuard,
});

const adminRoute = createRoute({
  getParentRoute: () => adminGuardRoute,
  path: "/admin",
  component: lazyRouteComponent(() => import("@/pages/AdminPage"), "AdminPage"),
});

const adminUserHistoryRoute = createRoute({
  getParentRoute: () => adminGuardRoute,
  path: "/admin/users/$userId/history",
  component: lazyRouteComponent(
    () => import("@/pages/AdminUserHistoryPage"),
    "AdminUserHistoryPage",
  ),
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  registerRoute,
  authRoute.addChildren([
    shellRoute.addChildren([
      indexRoute,
      forYouRoute,
      moodsRoute,
      searchRoute,
      seriesAliasRoute,
      collectionRoute,
      libraryRoute,
      watchRoute,
      historyRoute,
      accountRoute,
      adminGuardRoute.addChildren([adminRoute, adminUserHistoryRoute]),
    ]),
  ]),
]);

export const router = createRouter({
  routeTree,
  // Prefetch a route's code/data on link hover/focus — cheap SPA win, and it
  // covers the code-split chunks so navigations feel instant after intent.
  defaultPreload: "intent",
  defaultPendingComponent: RoutePending,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
