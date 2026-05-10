import { BrowserRouter, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider } from "@/lib/auth";
import { ThemeProvider } from "@/lib/theme";
import { AppShell } from "@/components/AppShell";
import { RequireAuth } from "@/components/RequireAuth";
import { LoginPage } from "@/pages/LoginPage";
import { RegisterPage } from "@/pages/RegisterPage";
import { HomePage } from "@/pages/HomePage";
import { SearchPage } from "@/pages/SearchPage";
import { SeriesPage } from "@/pages/SeriesPage";
import { CollectionPage } from "@/pages/CollectionPage";
import { AdminPage } from "@/pages/AdminPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { WatchPage } from "@/pages/WatchPage";
import { AccountPage } from "@/pages/AccountPage";

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
                  <Route path="/search" element={<SearchPage />} />
                  <Route path="/series/:followId" element={<SeriesPage />} />
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
          </BrowserRouter>
        </AuthProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
