/**
 * Canvas2D-based `VideoFrame` renderer for Tier C/D.
 *
 * `drawImage(VideoFrame, ...)` is universally supported, single-line, and
 * "fast enough" for 1080p on any 2020+ laptop. The WebGPU renderer takes
 * over where available (zero-copy import, HDR tone-mapping).
 *
 * Frames wait in the shared `FrameQueue` and a `requestAnimationFrame` loop
 * (running only while frames are queued) draws the one the AV-sync clock has
 * reached (audio is the master). Late frames are dropped; early frames stay
 * in the queue until their time arrives.
 */

import { drawLoop, FrameQueue } from './frame-queue';

export type Canvas2dRenderer = {
	/** Push a decoded frame. The renderer takes ownership and will
	 *  `close()` it after rendering or dropping. */
	enqueue: (frame: VideoFrame) => void;
	/** How many frames are sitting in the wait-to-render queue. */
	queueDepth: () => number;
	/** Drops every queued frame. */
	clear: () => void;
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
	const ctx = opts.canvas.getContext('2d', { alpha: false });
	if (!ctx) {
		throw new Error('Canvas2D context unavailable');
	}
	const queue = new FrameQueue<VideoFrame>({ lateMs: opts.lateMs ?? 80 });
	let intrinsic: { width: number; height: number } | null = null;
	let disposed = false;
	let lastDrawn = 0;

	// one draw per animation frame; the next one picks the next frame
	const loop = drawLoop(() => {
		const frame = queue.take(opts.clockSeconds());
		if (frame) {
			try {
				ctx.drawImage(frame, 0, 0, opts.canvas.width, opts.canvas.height);
				lastDrawn = frame.timestamp / 1_000_000;
			} catch (e) {
				opts.onError?.(e instanceof Error ? e : new Error(String(e)));
			} finally {
				frame.close();
			}
		}
		return queue.depth > 0;
	});

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
		loop.kick();
	};

	const dispose = (): void => {
		if (disposed) return;
		disposed = true;
		loop.stop();
		queue.clear();
	};

	return {
		enqueue,
		queueDepth: () => queue.depth,
		clear: () => queue.clear(),
		lastDrawnTs: () => lastDrawn,
		intrinsicSize: () => intrinsic,
		dispose
	};
}
