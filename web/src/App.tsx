import { BrowserRouter, Navigate, Route, Routes, useParams } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider } from "@/lib/auth";
import { ThemeProvider } from "@/lib/theme";
import { AppShell } from "@/components/AppShell";
import { ClientOutdatedOverlay } from "@/components/ClientOutdatedOverlay";
import { UpdateBanner } from "@/components/UpdateBanner";
import { RequireAuth } from "@/components/RequireAuth";
import { LoginPage } from "@/pages/LoginPage";
import { RegisterPage } from "@/pages/RegisterPage";
import { HomePage } from "@/pages/HomePage";
import { ForYouPage } from "@/pages/ForYouPage";
import { SearchPage } from "@/pages/SearchPage";
import { CollectionPage } from "@/pages/CollectionPage";
import { AdminPage } from "@/pages/AdminPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { WatchPage } from "@/pages/WatchPage";
import { AccountPage } from "@/pages/AccountPage";

/// Legacy `/series/:followId` route — kept as a redirect so any
/// bookmarks / shared links pinned before the unification still
/// resolve. The C1 façade returns `collection.id` as the follow
/// id, so the redirect is a straight pass-through; ids that don't
/// resolve to a collection 404 inside CollectionPage (acceptable
/// regression for stale series_follows rows).
function SeriesAlias() {
  const { followId } = useParams<{ followId: string }>();
  return <Navigate to={`/collection/${followId ?? ""}`} replace />;
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: false, refetchOnWindowFocus: false },
  },
});

export default function App() {
  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <AuthProvider>
          <BrowserRouter>
            <Routes>
              <Route path="/login" element={<LoginPage />} />
              <Route path="/register" element={<RegisterPage />} />
              <Route element={<RequireAuth />}>
                <Route element={<AppShell />}>
                  <Route index element={<HomePage />} />
                  <Route path="/for-you" element={<ForYouPage />} />
                  <Route path="/search" element={<SearchPage />} />
                  <Route path="/series/:followId" element={<SeriesAlias />} />
                  <Route path="/collection/:id" element={<CollectionPage />} />
                  <Route path="/library" element={<LibraryPage />} />
                  <Route path="/watch/:infohash/:idx" element={<WatchPage />} />
                  <Route path="/account" element={<AccountPage />} />
                  <Route element={<RequireAuth adminOnly />}>
                    <Route path="/admin" element={<AdminPage />} />
                  </Route>
                </Route>
              </Route>
            </Routes>
            <ClientOutdatedOverlay />
            <UpdateBanner />
          </BrowserRouter>
        </AuthProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
