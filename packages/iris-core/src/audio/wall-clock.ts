/**
 * The master clock of a canvas engine with no audio track: media time advancing with the wall
 * clock from an anchor, frozen while paused. (With audio, the scheduler's consumption-based
 * clock is the master.)
 */

export class WallClock {
	#now: () => number;
	/** Media time at `#since`, or null until anchored. */
	#base: number | null = null;
	#since = 0;
	#paused = false;

	/** `now` in milliseconds (`performance.now` in the engine). */
	constructor(now: () => number) {
		this.#now = now;
	}

	get anchored(): boolean {
		return this.#base !== null;
	}

	/** Media time in seconds (0 until anchored). */
	time(): number {
		if (this.#base === null) return 0;
		return this.#paused ? this.#base : this.#base + (this.#now() - this.#since) / 1000;
	}

	/** Starts counting from `mediaTime` (the first frame after a mount or a seek). */
	anchor(mediaTime: number): void {
		this.#base = mediaTime;
		this.#since = this.#now();
	}

	reset(): void {
		this.#base = null;
	}

	pause(): void {
		if (this.#paused) return;
		if (this.#base !== null) this.#base = this.time();
		this.#paused = true;
	}

	resume(): void {
		if (!this.#paused) return;
		this.#paused = false;
		this.#since = this.#now();
	}
}
