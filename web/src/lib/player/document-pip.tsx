/**
 * Document Picture-in-Picture. Renders the player into a real OS-level
 * always-on-top window via React `createPortal`, keeping the React
 * tree intact instead of physically moving DOM nodes (which fights
 * React's reconciler).
 *
 * Hook usage:
 * ```tsx
 * const pip = useDocumentPip();
 * return pip.renderInto(<PlayerContent />);
 * // <button onClick={pip.toggle}> uses the same controller.
 * ```
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

declare global {
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

export type DocumentPipController = {
  isActive: boolean;
  toggle: () => Promise<void>;
  /** Wrap your player JSX with this. Renders in-place when PiP is
   *  inactive, or via `createPortal` into the PiP window otherwise. */
  renderInto: (children: React.ReactNode) => React.ReactNode;
};

export function isDocumentPipSupported(): boolean {
  return typeof window !== "undefined" && "documentPictureInPicture" in window;
}

export function useDocumentPip(opts?: { width?: number; height?: number }): DocumentPipController {
  const [pipWindow, setPipWindow] = useState<Window | null>(null);
  const lastSizeRef = useRef<{ w: number; h: number } | null>(null);

  // Stylesheet adoption when the PiP window opens.
  useEffect(() => {
    if (!pipWindow) return;
    // 1. Try `adoptedStyleSheets` (Constructable Stylesheets) for any
    //    constructed sheets the app uses. Then copy the rest as text.
    try {
      const constructed: CSSStyleSheet[] = [];
      const inline: string[] = [];
      for (const sheet of Array.from(document.styleSheets)) {
        try {
          // Same-origin sheets: copy as text for portability across
          // Vite dev / prod plus bundled stylesheets.
          const text = Array.from(sheet.cssRules)
            .map((r) => r.cssText)
            .join("\n");
          if (text) inline.push(text);
        } catch {
          // Cross-origin sheet (rare for our same-origin setup): copy
          // via <link rel=stylesheet> if a URL is available.
          if (sheet.href) {
            const link = pipWindow.document.createElement("link");
            link.rel = "stylesheet";
            link.href = sheet.href;
            pipWindow.document.head.appendChild(link);
          }
        }
        void constructed;
      }
      if (inline.length > 0) {
        const style = pipWindow.document.createElement("style");
        style.textContent = inline.join("\n");
        pipWindow.document.head.appendChild(style);
      }
    } catch (e) {
      console.warn("[iris-core] PiP stylesheet adoption failed:", e);
    }
    // 2. Body baseline so the player renders correctly.
    pipWindow.document.body.style.margin = "0";
    pipWindow.document.body.style.padding = "0";
    pipWindow.document.body.style.background = "black";
    pipWindow.document.body.style.minHeight = "100vh";
    pipWindow.document.body.style.display = "flex";
    pipWindow.document.documentElement.style.height = "100%";
    pipWindow.document.body.style.height = "100%";
  }, [pipWindow]);

  // Close handling.
  useEffect(() => {
    if (!pipWindow) return;
    const onClose = () => setPipWindow(null);
    pipWindow.addEventListener("pagehide", onClose);
    return () => pipWindow.removeEventListener("pagehide", onClose);
  }, [pipWindow]);

  const toggle = useCallback(async () => {
    if (pipWindow) {
      try {
        pipWindow.close();
      } catch {
        /* idempotent */
      }
      setPipWindow(null);
      return;
    }
    if (!isDocumentPipSupported()) {
      console.warn("[iris-core] Document PiP not supported on this browser");
      return;
    }
    const api = window.documentPictureInPicture;
    if (!api) return;
    const size = lastSizeRef.current ?? {
      w: opts?.width ?? 720,
      h: opts?.height ?? 405,
    };
    try {
      const win = await api.requestWindow({
        width: size.w,
        height: size.h,
        disallowReturnToOpener: false,
        preferInitialWindowPlacement: true,
      });
      setPipWindow(win);
    } catch (e) {
      console.warn("[iris-core] Document PiP requestWindow failed:", e);
    }
  }, [pipWindow, opts?.width, opts?.height]);

  const renderInto = useCallback(
    (children: React.ReactNode): React.ReactNode => {
      if (!pipWindow) return children;
      return createPortal(children, pipWindow.document.body);
    },
    [pipWindow],
  );

  return {
    isActive: pipWindow != null,
    toggle,
    renderInto,
  };
}
