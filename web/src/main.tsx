import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";

// Stale-chunk recovery. After a redeploy, an open tab still holds the
// OLD `index.html` and its hashed chunk filenames (e.g.
// `tier-b-mse-Xa3bF1.js`). The next `import()` the user triggers — a
// tier swap, opening a route that's code-split, mounting a libpgs
// overlay — fetches a chunk URL the new server doesn't ship anymore
// and rejects with "Failed to fetch dynamically imported module".
// Vite emits `vite:preloadError` on `window` for exactly this case.
//
// We force a full reload, which pulls the new HTML + new chunk
// filenames. `sessionStorage` guards against an infinite reload loop
// if the reload itself fails to bring the app back (e.g. the user is
// genuinely offline / the deploy is broken) — within 10 s of the last
// stale-reload attempt we surface the error instead of looping.
const STALE_RELOAD_KEY = "iris.staleReloadAt";
window.addEventListener("vite:preloadError", (event) => {
  const last = Number(sessionStorage.getItem(STALE_RELOAD_KEY) ?? "0");
  const now = Date.now();
  if (now - last < 10_000) {
    console.error(
      "[iris] preload error after recent reload — leaving page intact",
      event,
    );
    return;
  }
  sessionStorage.setItem(STALE_RELOAD_KEY, String(now));
  console.warn("[iris] stale bundle detected, reloading to pick up new build");
  window.location.reload();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
