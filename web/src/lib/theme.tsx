import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type Theme = "light" | "dark" | "system";

/** Key shared with the inline boot script in `index.html`. */
export const THEME_STORAGE_KEY = "iris-theme";

type ThemeContextValue = {
  theme: Theme;
  /** What's actually applied on the document right now: light or dark. */
  resolved: "light" | "dark";
  setTheme: (t: Theme) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readStored(): Theme {
  if (typeof window === "undefined") return "system";
  const v = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (v === "light" || v === "dark" || v === "system") return v;
  return "system";
}

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolve(t: Theme): "light" | "dark" {
  if (t === "system") return systemPrefersDark() ? "dark" : "light";
  return t;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => readStored());
  const [resolved, setResolved] = useState<"light" | "dark">(() => resolve(readStored()));

  // Apply class on html + listen to system changes (only when theme=system).
  useEffect(() => {
    const apply = () => {
      const next = resolve(theme);
      setResolved(next);
      document.documentElement.classList.toggle("dark", next === "dark");
    };
    apply();
    if (theme !== "system") return;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    mql.addEventListener("change", apply);
    return () => mql.removeEventListener("change", apply);
  }, [theme]);

  const setAndPersist = useCallback((t: Theme) => {
    window.localStorage.setItem(THEME_STORAGE_KEY, t);
    setTheme(t);
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({ theme, resolved, setTheme: setAndPersist }),
    [theme, resolved, setAndPersist],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used inside <ThemeProvider>");
  return ctx;
}
