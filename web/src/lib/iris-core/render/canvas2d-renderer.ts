/**
 * Canvas2D-based `VideoFrame` renderer for Tier C/D.
 *
 * Phase 2b alpha: `drawImage(VideoFrame, ...)` is universally
 * supported, single-line, and "fast enough" for 1080p on any 2020+
 * laptop. WebGPU's `importExternalTexture` zero-copy path lands in
 * Phase 2b-beta when we wire HDR.
 *
 * The renderer maintains a small frame queue and a `requestAnimationFrame`
 * loop that picks the right frame to draw based on the AV-sync clock
 * (audio is the master). Late frames are dropped; early frames stay in
 * the queue until their wall-clock time arrives.
 */

export type Canvas2dRenderer = {
  /** Push a decoded frame. The renderer takes ownership and will
   *  `close()` it after rendering or dropping. */
  enqueue: (frame: VideoFrame) => void;
  /** How many frames are sitting in the wait-to-render queue. */
  queueDepth: () => number;
  /** Timestamp (seconds) of the last frame actually drawn — ground
   *  truth for "what the viewer's eye is seeing right now". */
  lastDrawnTs: () => number;
  /** Resize the canvas to the frame's intrinsic size when the first
   *  frame arrives. Returns the resolved size or `null` if no frame
   *  has been rendered yet. */
  intrinsicSize: () => { width: number; height: number } | null;
  /** Stop the rAF loop, drop pending frames, leave the canvas as-is. */
  dispose: () => void;
};

export type Canvas2dRendererOptions = {
  canvas: HTMLCanvasElement;
  /** Returns the current AV-master clock in seconds. Frames whose
   *  presentation time is more than `lateMs` ms behind this are
   *  dropped. */
  clockSeconds: () => number;
  lateMs?: number;
  onError?: (err: Error) => void;
};

export function createCanvas2dRenderer(opts: Canvas2dRendererOptions): Canvas2dRenderer {
  const ctx = opts.canvas.getContext("2d", { alpha: false });
  if (!ctx) {
    throw new Error("Canvas2D context unavailable");
  }
  const queue: VideoFrame[] = [];
  let intrinsic: { width: number; height: number } | null = null;
  let disposed = false;
  let lastDrawn = 0;
  const lateMs = opts.lateMs ?? 80;

  const enqueue = (frame: VideoFrame): void => {
    if (disposed) {
      frame.close();
      return;
    }
    if (!intrinsic) {
      intrinsic = { width: frame.displayWidth, height: frame.displayHeight };
      opts.canvas.width = intrinsic.width;
      opts.canvas.height = intrinsic.height;
    }
    queue.push(frame);
    // Keep the queue bounded — anything beyond 32 frames suggests the
    // renderer is starved and the decoder is over-producing. Drop the
    // oldest excess to bound memory.
    while (queue.length > 32) {
      const dropped = queue.shift();
      dropped?.close();
    }
  };

  const tick = (): void => {
    if (disposed) return;
    const now = opts.clockSeconds();
    while (queue.length > 0) {
      const head = queue[0];
      if (!head) break;
      const headTs = head.timestamp / 1_000_000;
      if (headTs > now + 0.001) {
        // Too early — wait for the clock to catch up.
        break;
      }
      // Drop heavily-late frames (clock passed them by more than lateMs)
      // unless this is the only frame we've got.
      const lateBy = (now - headTs) * 1000;
      if (lateBy > lateMs && queue.length > 1) {
        const dropped = queue.shift();
        dropped?.close();
        continue;
      }
      // Draw this frame and consume it.
      const drawn = queue.shift();
      if (!drawn) break;
      try {
        ctx.drawImage(drawn, 0, 0, opts.canvas.width, opts.canvas.height);
        lastDrawn = drawn.timestamp / 1_000_000;
      } catch (e) {
        opts.onError?.(e instanceof Error ? e : new Error(String(e)));
      } finally {
        drawn.close();
      }
      // Only one draw per rAF tick — the next rAF will pick the next frame.
      break;
    }
    if (!disposed) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  const dispose = (): void => {
    if (disposed) return;
    disposed = true;
    for (const f of queue) f.close();
    queue.length = 0;
  };

  return {
    enqueue,
    queueDepth: () => queue.length,
    lastDrawnTs: () => lastDrawn,
    intrinsicSize: () => intrinsic,
    dispose,
  };
}
