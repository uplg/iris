/**
 * Unified `VideoRenderer` interface. Both Canvas2D and WebGPU
 * implementations live behind this shape; `mountRenderer` picks the
 * best available path based on `navigator.gpu` presence + the user's
 * `hdr` preference.
 *
 * The renderer owns its own `<canvas>` and inserts it into the
 * container passed by the caller. The handle's `canvas` field is
 * exposed so `Document PiP` can wire `canvas.captureStream()`.
 */

import type { Canvas2dRenderer } from "./canvas2d-renderer";

export type RendererHdrMode = "sdr" | "auto";

export type VideoRendererOptions = {
  container: HTMLDivElement;
  /** Returns the current master-clock seconds. Renderer drops late
   *  frames and waits on early ones. */
  clockSeconds: () => number;
  /** `"auto"` uses an HDR canvas when the source + display support it
   *  (WebGPU only). `"sdr"` forces 8-bit linear sRGB output. */
  hdr: RendererHdrMode;
  onError?: (err: Error) => void;
};

export type VideoRenderer = {
  /** Push a decoded frame. The renderer takes ownership. */
  enqueue: (frame: VideoFrame) => void;
  /** Frames queued waiting to render. */
  queueDepth: () => number;
  /** First frame's intrinsic size or null. */
  intrinsicSize: () => { width: number; height: number } | null;
  /** The output canvas (Document PiP wires `captureStream()`). */
  canvas: HTMLCanvasElement;
  /** True if WebGPU is in use (HDR-capable + zero-copy import). */
  isHardwareAccelerated: () => boolean;
  dispose: () => void;
};

/** Mount the best available renderer for this browser + display. */
export async function mountRenderer(opts: VideoRendererOptions): Promise<VideoRenderer> {
  // WebGPU first when the API is present. The renderer's own probe
  // (device adapter request) can still fail; we catch + fall back.
  const hasWebGpu = typeof (navigator as Navigator & { gpu?: unknown }).gpu !== "undefined";
  if (hasWebGpu) {
    try {
      const { mountWebGpuRenderer } = await import("./webgpu-renderer");
      return await mountWebGpuRenderer(opts);
    } catch (e) {
      console.warn("[iris-core] WebGPU renderer unavailable, falling back to Canvas2D:", e);
    }
  }
  return mountCanvas2d(opts);
}

async function mountCanvas2d(opts: VideoRendererOptions): Promise<VideoRenderer> {
  const { createCanvas2dRenderer } = await import("./canvas2d-renderer");
  const canvas = document.createElement("canvas");
  canvas.className = "h-full w-full object-contain bg-black";
  opts.container.appendChild(canvas);
  const inner: Canvas2dRenderer = createCanvas2dRenderer({
    canvas,
    clockSeconds: opts.clockSeconds,
    onError: opts.onError,
  });
  return {
    enqueue: inner.enqueue,
    queueDepth: inner.queueDepth,
    intrinsicSize: inner.intrinsicSize,
    canvas,
    isHardwareAccelerated: () => false,
    dispose: () => {
      inner.dispose();
      if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
    },
  };
}
