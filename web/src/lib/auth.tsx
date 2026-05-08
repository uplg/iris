import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { auth as authApi, type User } from "./api";

type AuthState =
  | { status: "loading" }
  | { status: "anonymous" }
  | { status: "authenticated"; user: User };

type AuthContextValue = AuthState & {
  login: (email: string, password: string) => Promise<void>;
  register: (token: string, email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
};

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ status: "loading" });

  const refresh = useCallback(async () => {
    try {
      const user = await authApi.me();
      setState({ status: "authenticated", user });
    } catch {
      try {
        const user = await authApi.refresh();
        setState({ status: "authenticated", user });
      } catch {
        setState({ status: "anonymous" });
      }
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Keep-alive: while authenticated, periodically rotate the access cookie
  // by hitting /auth/refresh. Without this, hls.js segment requests silently
  // 401 once the access token expires mid-stream. Server access TTL is 1h,
  // we rotate every 25 min so we always have margin.
  useEffect(() => {
    if (state.status !== "authenticated") return;
    const interval = window.setInterval(
      () => {
        void authApi
          .refresh()
          .then((user) => setState({ status: "authenticated", user }))
          .catch(() => {
            // Refresh token expired or revoked → bounce to login.
            setState({ status: "anonymous" });
          });
      },
      25 * 60_000,
    );
    return () => window.clearInterval(interval);
  }, [state.status]);

  const login = useCallback(async (email: string, password: string) => {
    const user = await authApi.login(email, password);
    setState({ status: "authenticated", user });
  }, []);

  const register = useCallback(async (token: string, email: string, password: string) => {
    const user = await authApi.register(token, email, password);
    setState({ status: "authenticated", user });
  }, []);

  const logout = useCallback(async () => {
    await authApi.logout();
    setState({ status: "anonymous" });
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({ ...state, login, register, logout, refresh }),
    [state, login, register, logout, refresh],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used inside <AuthProvider>");
  return ctx;
}
