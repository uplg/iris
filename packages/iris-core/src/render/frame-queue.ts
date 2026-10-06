/**
 * Decoded frames waiting for their presentation time, shared by the canvas renderers (each only
 * draws). The decoder is paced on the media clock (Tier C), so the queue holds a fraction of a
 * second; the cap is a memory safety net, not the flow control.
 */

export type TimedFrame = { readonly timestamp: number; close(): void };

export type FrameQueueOptions = {
	/** Frames kept at most; beyond it the oldest go. */
	max?: number;
	/** A frame this late (ms) behind the clock is skipped when a later one is waiting. */
	lateMs?: number;
};

export class FrameQueue<F extends TimedFrame> {
	#frames: F[] = [];
	#max: number;
	#lateMs: number;

	constructor(opts: FrameQueueOptions = {}) {
		this.#max = opts.max ?? 32;
		this.#lateMs = opts.lateMs ?? 80;
	}

	get depth(): number {
		return this.#frames.length;
	}

	push(frame: F): void {
		this.#frames.push(frame);
		while (this.#frames.length > this.#max) this.#frames.shift()?.close();
	}

	/** The frame to draw at `now` (seconds), handed over (the caller closes it); null while
	 *  the next frame is still early. Frames the clock left behind are closed on the way. */
	take(now: number): F | null {
		while (this.#frames.length > 0) {
			const head = this.#frames[0]!;
			const headTs = head.timestamp / 1_000_000;
			if (headTs > now + 0.001) return null;
			const lateBy = (now - headTs) * 1000;
			if (lateBy > this.#lateMs && this.#frames.length > 1) {
				this.#frames.shift()!.close();
				continue;
			}
			return this.#frames.shift()!;
		}
		return null;
	}

	clear(): void {
		for (const f of this.#frames) f.close();
		this.#frames.length = 0;
	}
}

/** A requestAnimationFrame loop that runs only while there is something to draw: woken by
 *  `kick()` when a frame arrives, asleep again once the queue is empty. Paused, the queued
 *  frames wait on a frozen clock, so it sleeps too: an arriving frame still gets one step (the
 *  picture of a seek made while paused), and `setPaused(false)` wakes it. */
export function drawLoop(
	step: () => boolean,
	raf: (cb: () => void) => number = requestAnimationFrame
): { kick: () => void; stop: () => void; setPaused: (paused: boolean) => void } {
	let scheduled = false;
	let stopped = false;
	let paused = false;
	const tick = () => {
		scheduled = false;
		if (stopped) return;
		if (step() && !paused) {
			scheduled = true;
			raf(tick);
		}
	};
	const kick = () => {
		if (scheduled || stopped) return;
		scheduled = true;
		raf(tick);
	};
	return {
		kick,
		stop: () => {
			stopped = true;
		},
		setPaused: (next) => {
			if (paused === next) return;
			paused = next;
			if (!paused) kick();
		}
	};
}
