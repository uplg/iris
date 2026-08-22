// Web Storage is not merely absent when the browser blocks site data (Safari
// private mode, "block all cookies", a locked-down webview): *touching*
// `localStorage` throws `SecurityError`. A `typeof localStorage === "undefined"`
// guard doesn't catch that, and the theme provider reads it from a `useState`
// initialiser — one unguarded read white-screens the whole app before any
// error boundary exists. index.html's pre-hydration script carries the same
// try/catch for the same reason.

export function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeLocal(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage blocked or quota exhausted — the preference just doesn't persist.
  }
}

export function readSession(key: string): string | null {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeSession(key: string, value: string): void {
  try {
    sessionStorage.setItem(key, value);
  } catch {
    // See writeLocal.
  }
}
