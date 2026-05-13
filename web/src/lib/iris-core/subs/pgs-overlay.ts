/**
 * PGS bitmap subtitle overlay via `libpgs`.
 *
 * Cleaner integration than libass-wasm: `libpgs` is a proper ES module
 * with a worker shipped alongside. We pass it the host element + a
 * subtitle URL and it manages everything else (decode, scale, paint
 * sync via the supplied `<video>` element).
 *
 * For Tier C (canvas-based playback, no `<video>`) we synthesise a
 * minimal "video-like" interface via a hidden `<video>` and drive its
 * `currentTime` from the AV scheduler's clock. Cheap and lets us reuse
 * the same `PgsRenderer` code path.
 */

const WORKER_URL = "/libpgs/libpgs.worker.js";

export type PgsOverlayOptions = {
  host: HTMLElement;
  subUrl: string;
  getCurrentTime: () => number;
  /** Native `<video>` element to bind to. When omitted, we run a rAF
   *  loop that calls `renderAtTimestamp(getCurrentTime())`. */
  video?: HTMLVideoElement;
};

export type PgsOverlayHandle = {
  dispose: () => void;
};

export async function mountPgsOverlay(opts: PgsOverlayOptions): Promise<PgsOverlayHandle> {
  // libpgs is ~60 KB gzipped; deferring its load shaves it off the
  // initial bundle for users who never enable a PGS sub.
  const { PgsRenderer } = await import("libpgs");
  const canvas = document.createElement("canvas");
  canvas.className = "pointer-events-none absolute inset-0 h-full w-full";
  opts.host.appendChild(canvas);

  const renderer = new PgsRenderer({
    canvas,
    workerUrl: WORKER_URL,
    video: opts.video,
    aspectRatio: "contain",
  });
  renderer.loadFromUrl(opts.subUrl);

  let rafId: number | null = null;
  if (!opts.video) {
    const tick = () => {
      try {
        renderer.renderAtTimestamp(opts.getCurrentTime());
      } catch {
        /* ignore */
      }
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
  }

  return {
    dispose: () => {
      if (rafId !== null) cancelAnimationFrame(rafId);
      try {
        renderer.dispose();
      } catch {
        /* idempotent */
      }
      if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
    },
  };
}
