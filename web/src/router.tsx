import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import { AppShell } from "@/components/AppShell";
import { ClientOutdatedOverlay } from "@/components/ClientOutdatedOverlay";
import { RequireAuth } from "@/components/RequireAuth";
import { UpdateBanner } from "@/components/UpdateBanner";
import { AccountPage } from "@/pages/AccountPage";
import { AdminPage } from "@/pages/AdminPage";
import { CollectionPage } from "@/pages/CollectionPage";
import { ForYouPage } from "@/pages/ForYouPage";
import { HomePage } from "@/pages/HomePage";
import { LibraryPage } from "@/pages/LibraryPage";
import { LoginPage } from "@/pages/LoginPage";
import { RegisterPage } from "@/pages/RegisterPage";
import { SearchPage } from "@/pages/SearchPage";
import { WatchPage } from "@/pages/WatchPage";

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
  component: LoginPage,
});

const registerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/register",
  // `?token=…` pre-fills the invitation field from a shared invite link.
  validateSearch: (search: Record<string, unknown>): { token?: string } => ({
    token: str(search.token),
  }),
  component: RegisterPage,
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
  component: HomePage,
});

const forYouRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/for-you",
  component: ForYouPage,
});

const searchRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/search",
  validateSearch: (search: Record<string, unknown>): { q?: string } => ({
    q: str(search.q),
  }),
  component: SearchPage,
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
  component: CollectionPage,
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
  component: LibraryPage,
});

const watchRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/watch/$infohash/$idx",
  component: WatchPage,
});

const accountRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/account",
  // `?pair=ABCD-EFGH` deep-links a TV pairing code into the input.
  validateSearch: (search: Record<string, unknown>): { pair?: string } => ({
    pair: str(search.pair),
  }),
  component: AccountPage,
});

const adminGuardRoute = createRoute({
  getParentRoute: () => shellRoute,
  id: "adminGuard",
  component: AdminGuard,
});

const adminRoute = createRoute({
  getParentRoute: () => adminGuardRoute,
  path: "/admin",
  component: AdminPage,
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  registerRoute,
  authRoute.addChildren([
    shellRoute.addChildren([
      indexRoute,
      forYouRoute,
      searchRoute,
      seriesAliasRoute,
      collectionRoute,
      libraryRoute,
      watchRoute,
      accountRoute,
      adminGuardRoute.addChildren([adminRoute]),
    ]),
  ]),
]);

export const router = createRouter({
  routeTree,
  // Prefetch a route's code/data on link hover/focus — cheap SPA win.
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
