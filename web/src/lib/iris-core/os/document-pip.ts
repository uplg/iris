/**
 * Document Picture-in-Picture wiring. Shipped in Chromium 116+ and
 * WebKit / Safari 26+. Lets us pop the IrisPlayer wrapper out into a
 * real OS-level always-on-top window with our custom chrome still
 * intact — unlike the legacy `<video>` PiP, which strips the UI.
 *
 * Phase-2 polish wires the API behind a feature-detect; older browsers
 * can fall back to the per-tier `<video>` PiP via `requestPictureInPicture`.
 */

declare global {
  // Chrome's Document PiP API. Type definitions aren't in lib.dom.d.ts
  // as of TS 6.0; declare the narrow surface we use.
  interface DocumentPictureInPictureOptions {
    width?: number;
    height?: number;
    disallowReturnToOpener?: boolean;
    preferInitialWindowPlacement?: boolean;
  }
  interface DocumentPictureInPicture {
    window: Window | null;
    requestWindow(options?: DocumentPictureInPictureOptions): Promise<Window>;
  }
  interface Window {
    documentPictureInPicture?: DocumentPictureInPicture;
  }
}

export type DocumentPipHandle = {
  /** True while the player is in a PiP window. */
  isActive: () => boolean;
  /** Toggle PiP on/off. */
  toggle: () => Promise<void>;
  /** Subscribe to active-state changes. */
  onChange: (cb: (active: boolean) => void) => () => void;
};

export function isDocumentPipSupported(): boolean {
  return typeof window !== "undefined" && "documentPictureInPicture" in window;
}

/**
 * Mount a Document PiP controller for a given player wrapper. The
 * wrapper is *moved* into the PiP window's document on toggle-on, and
 * moved back to its original parent on toggle-off. CSS for the
 * IrisPlayer is portable enough that the same classes apply in either
 * window.
 */
export function mountDocumentPip(wrapper: HTMLElement): DocumentPipHandle {
  if (!isDocumentPipSupported()) {
    return {
      isActive: () => false,
      toggle: async () => undefined,
      onChange: () => () => undefined,
    };
  }

  const originalParent = wrapper.parentElement;
  const listeners = new Set<(active: boolean) => void>();
  let pipWindow: Window | null = null;
  const fire = (active: boolean) => {
    for (const cb of listeners) cb(active);
  };

  const close = (): void => {
    if (!pipWindow) return;
    // Move the wrapper back, then close the window.
    if (originalParent) originalParent.appendChild(wrapper);
    try {
      pipWindow.close();
    } catch {
      /* idempotent */
    }
    pipWindow = null;
    fire(false);
  };

  const toggle = async (): Promise<void> => {
    if (pipWindow) {
      close();
      return;
    }
    const api = window.documentPictureInPicture;
    if (!api) return;
    const rect = wrapper.getBoundingClientRect();
    const aspect = rect.height > 0 ? rect.width / rect.height : 16 / 9;
    const width = 720;
    const height = Math.round(width / aspect);
    try {
      pipWindow = await api.requestWindow({
        width,
        height,
        disallowReturnToOpener: false,
        preferInitialWindowPlacement: true,
      });
    } catch (e) {
      console.warn("[iris-core] Document PiP request failed:", e);
      return;
    }
    // Copy parent-document stylesheets into the PiP window so our
    // Tailwind classes resolve. The simplest portable approach is
    // adopt-stylesheets / copying the <style> tags.
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        const rules = Array.from(sheet.cssRules).map((r) => r.cssText).join("\n");
        const style = pipWindow.document.createElement("style");
        style.textContent = rules;
        pipWindow.document.head.appendChild(style);
      } catch {
        // Cross-origin stylesheets fail .cssRules access; copy via <link>.
        if (sheet.href) {
          const link = pipWindow.document.createElement("link");
          link.rel = "stylesheet";
          link.href = sheet.href;
          pipWindow.document.head.appendChild(link);
        }
      }
    }
    pipWindow.document.body.style.margin = "0";
    pipWindow.document.body.style.background = "black";
    pipWindow.document.body.appendChild(wrapper);
    pipWindow.addEventListener("pagehide", close);
    fire(true);
  };

  return {
    isActive: () => pipWindow != null,
    toggle,
    onChange: (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
