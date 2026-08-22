import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { readLocal, writeLocal } from "./safe-storage";

export type Theme = "light" | "dark" | "system";
export type Accent = "violet" | "indigo" | "emerald" | "amber" | "rose";
export type Density = "comfortable" | "compact";

export const ACCENTS: Accent[] = ["violet", "indigo", "emerald", "amber", "rose"];

/** A swatch colour per accent, used by the picker UI. Matches the
 *  `[data-accent]` blocks in index.css. */
export const ACCENT_SWATCH: Record<Accent, string> = {
  violet: "oklch(0.72 0.18 290)",
  indigo: "oklch(0.65 0.2 270)",
  emerald: "oklch(0.72 0.16 165)",
  amber: "oklch(0.78 0.16 70)",
  rose: "oklch(0.7 0.2 10)",
};

/** Keys shared with the inline boot script in `index.html`. */
export const THEME_STORAGE_KEY = "iris-theme";
export const ACCENT_STORAGE_KEY = "iris-accent";
export const DENSITY_STORAGE_KEY = "iris-density";

type ThemeContextValue = {
  theme: Theme;
  /** What's actually applied on the document right now: light or dark. */
  resolved: "light" | "dark";
  setTheme: (t: Theme) => void;
  accent: Accent;
  setAccent: (a: Accent) => void;
  density: Density;
  setDensity: (d: Density) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readStored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const v = readLocal(key);
  return allowed.includes(v as T) ? (v as T) : fallback;
}

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolve(t: Theme): "light" | "dark" {
  if (t === "system") return systemPrefersDark() ? "dark" : "light";
  return t;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() =>
    readStored(THEME_STORAGE_KEY, ["light", "dark", "system"], "system"),
  );
  const [resolved, setResolved] = useState<"light" | "dark">(() => resolve(theme));
  const [accent, setAccent] = useState<Accent>(() =>
    readStored(ACCENT_STORAGE_KEY, ACCENTS, "violet"),
  );
  const [density, setDensity] = useState<Density>(() =>
    readStored(DENSITY_STORAGE_KEY, ["comfortable", "compact"], "comfortable"),
  );

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

  // Reflect accent + density onto <html> data-* attributes.
  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);
  useEffect(() => {
    document.documentElement.dataset.density = density;
  }, [density]);

  const setAndPersist = useCallback((t: Theme) => {
    writeLocal(THEME_STORAGE_KEY, t);
    setTheme(t);
  }, []);
  const setAccentAndPersist = useCallback((a: Accent) => {
    writeLocal(ACCENT_STORAGE_KEY, a);
    setAccent(a);
  }, []);
  const setDensityAndPersist = useCallback((d: Density) => {
    writeLocal(DENSITY_STORAGE_KEY, d);
    setDensity(d);
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({
      theme,
      resolved,
      setTheme: setAndPersist,
      accent,
      setAccent: setAccentAndPersist,
      density,
      setDensity: setDensityAndPersist,
    }),
    [theme, resolved, setAndPersist, accent, setAccentAndPersist, density, setDensityAndPersist],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used inside <ThemeProvider>");
  return ctx;
}
