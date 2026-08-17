import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { ApiError, AUTH_EXPIRED_EVENT, auth as authApi, type User } from "./api";

type AuthState =
  | { status: "loading"; retrying: boolean }
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
  const [state, setState] = useState<AuthState>({ status: "loading", retrying: false });

  // Idempotent "logged out" settle. The bootstrap catch and the
  // AUTH_EXPIRED_EVENT listener can both fire in the same tick; keeping the
  // state object's identity stable avoids gratuitous context re-renders.
  const settleAnonymous = useCallback(() => {
    setState((prev) => (prev.status === "anonymous" ? prev : { status: "anonymous" }));
  }, []);

  // One session-bootstrap attempt. "transient" = neither call reached an auth
  // verdict (timeout, network error, 429/5xx) — the cookies may still be
  // valid, so the caller must retry rather than bounce a live session to
  // /login. Only an explicit 401/403 from the refresh settles as anonymous.
  const bootstrap = useCallback(async (): Promise<"settled" | "transient"> => {
    try {
      const user = await authApi.me();
      setState({ status: "authenticated", user });
      return "settled";
    } catch {
      try {
        const user = await authApi.refresh();
        setState({ status: "authenticated", user });
        return "settled";
      } catch (e) {
        if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
          settleAnonymous();
          return "settled";
        }
        return "transient";
      }
    }
  }, [settleAnonymous]);

  // Bootstrap with backoff retry (timer approved — CLAUDE.md web-timer rule):
  // a transient failure keeps the boot screen up and retries instead of
  // hanging on a stalled fetch forever.
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const run = async (delayMs: number) => {
      if ((await bootstrap()) === "transient" && !cancelled) {
        setState({ status: "loading", retrying: true });
        timer = window.setTimeout(() => void run(Math.min(delayMs * 2, 30_000)), delayMs);
      }
    };
    void run(2_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [bootstrap]);

  const refresh = useCallback(async () => {
    await bootstrap();
  }, [bootstrap]);

  // api.ts dispatches this when an authenticated request gets 401 and
  // the refresh attempt also fails — the user is effectively logged out.
  // Flip to `anonymous` here so RequireAuth redirects to /login instead
  // of letting React Query render the raw "Unauthorized" message.
  useEffect(() => {
    window.addEventListener(AUTH_EXPIRED_EVENT, settleAnonymous);
    return () => window.removeEventListener(AUTH_EXPIRED_EVENT, settleAnonymous);
  }, [settleAnonymous]);

  // Keep-alive: while authenticated, periodically rotate the access cookie
  // by hitting /auth/refresh. Without this, byte-range requests for the
  // playback file silently 401 once the access token expires mid-stream.
  // Server access TTL is 1h; we rotate every 25 min for margin.
  useEffect(() => {
    if (state.status !== "authenticated") return;
    const interval = window.setInterval(() => {
      void authApi
        .refresh()
        .then((user) => setState({ status: "authenticated", user }))
        .catch((e: unknown) => {
          // Only a genuine auth death (refresh token expired/revoked → 401/403)
          // logs the user out. A transient failure — rate-limit (429), a 5xx
          // during a redeploy, or a network blip — must NOT: the session is
          // still valid and the next tick (or any user action) recovers. The
          // old code bounced to login on ANY rejection, which is the main
          // "logged out for no reason" path.
          if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
            settleAnonymous();
          }
        });
    }, 25 * 60_000);
    return () => window.clearInterval(interval);
  }, [state.status, settleAnonymous]);

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
