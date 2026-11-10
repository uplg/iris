/**
 * The muxer's chunks on their way into one SourceBuffer. `appendBuffer` takes one chunk at a
 * time, so they queue here and `pump` hands over the next one whenever the buffer is idle; the
 * engine pumps from its own events (`updateend`, `timeupdate`, a stall). A QuotaExceededError
 * keeps the chunk at the head for a later pump and lets the engine free room: no retry loop,
 * the next event retries.
 *
 * It also knows how long the SourceBuffer's current op (an append, or the engine's own
 * `remove`) has been in flight, which is what tells a hung op from one merely under way.
 */

/** An op this long in flight is hung: a healthy append or remove settles in milliseconds.
 *  `updating` alone says nothing. A `waiting` fired by a seek lands while the first appends
 *  of the new run are in flight, and aborting those throws their fragments away (WebKit
 *  then starves at the seek point, stall after stall). */
export const WEDGED_OP_MS = 2_000;

export type AppendQueueOptions = {
	/** False once the engine is disposed: nothing more is appended. */
	alive: () => boolean;
	/** The browser's byte ceiling was hit; the chunk stays queued. */
	onQuota: () => void;
	onError: (err: Error) => void;
	/** An append was handed to the SourceBuffer. */
	onAppend?: () => void;
	/** The hevc.js proxy transfers what it is given to its worker: hand it a private copy, and
	 *  serialise on our own flag (it keeps `updating` false while it works). */
	proxied?: boolean;
	/** Proxied mode: an append settled (landed, failed or aborted). */
	onSettled?: () => void;
};

export class AppendQueue {
	readonly chunks: Uint8Array[] = [];
	#sb: SourceBuffer | null = null;
	#inFlight = false;
	#opSince = 0;
	#opts: AppendQueueOptions;
	#opStarted = () => {
		this.#opSince = performance.now();
	};
	#opSettled = () => {
		// the engine's own `updateend` listener may have pumped the next append already
		if (!this.#sb?.updating) this.#opSince = 0;
	};
	#settle = () => {
		this.#inFlight = false;
		this.#opts.onSettled?.();
		this.pump();
	};

	constructor(opts: AppendQueueOptions) {
		this.#opts = opts;
	}

	/** Starts feeding `sb` (the queue may fill before the SourceBuffer exists). */
	attach(sb: SourceBuffer): void {
		this.detach();
		this.#sb = sb;
		this.#inFlight = false;
		this.#opSince = 0;
		sb.addEventListener('updatestart', this.#opStarted);
		sb.addEventListener('updateend', this.#opSettled);
		if (this.#opts.proxied) {
			for (const e of ['updateend', 'error', 'abort']) sb.addEventListener(e, this.#settle);
		}
	}

	detach(): void {
		this.#sb?.removeEventListener('updatestart', this.#opStarted);
		this.#sb?.removeEventListener('updateend', this.#opSettled);
		if (this.#sb && this.#opts.proxied) {
			for (const e of ['updateend', 'error', 'abort']) this.#sb.removeEventListener(e, this.#settle);
		}
		this.#sb = null;
	}

	/** How long the SourceBuffer's current op has been in flight; 0 when it is idle. */
	get opAgeMs(): number {
		if (!this.#sb?.updating) return 0;
		// an op whose `updatestart` was never seen ages from the first look at it
		if (this.#opSince === 0) this.#opSince = performance.now();
		return performance.now() - this.#opSince;
	}

	/** Recovery for a hung op (Firefox bug 1120084: `updating` stays true, no `updateend`,
	 *  no `error`). `abort()` is the spec's reset: it ends the running algorithm, clears
	 *  `updating` and resets the segment parser so the next append lands; the half-parsed
	 *  fragment is lost, a gap for the engine to jump. Call it from a stall: an op younger
	 *  than `WEDGED_OP_MS` is left alone. True when an op was aborted. */
	unwedge(): boolean {
		const sb = this.#sb;
		if (!sb || this.opAgeMs < WEDGED_OP_MS) return false;
		try {
			sb.abort();
		} catch {
			// MediaSource not open (the dispose path owns it), or a remove, which `abort` refuses
			return false;
		}
		this.#opSince = 0;
		return true;
	}

	get length(): number {
		return this.chunks.length;
	}

	push(...parts: Uint8Array[]): void {
		this.chunks.push(...parts);
	}

	clear(): void {
		this.chunks.length = 0;
	}

	/** Appends the next chunk if the SourceBuffer is idle. True when one was handed over. */
	pump(): boolean {
		const sb = this.#sb;
		if (!sb || !this.#opts.alive() || this.#inFlight || sb.updating) return false;
		const next = this.chunks.shift();
		if (!next) return false;
		try {
			// a view is fine: `appendBuffer` copies synchronously
			sb.appendBuffer((this.#opts.proxied ? next.slice() : next) as Uint8Array<ArrayBuffer>);
		} catch (e) {
			if (e instanceof DOMException && e.name === 'QuotaExceededError') {
				this.chunks.unshift(next);
				this.#opts.onQuota();
				return false;
			}
			this.#opts.onError(e instanceof Error ? e : new Error(String(e)));
			return false;
		}
		if (this.#opts.proxied) this.#inFlight = true;
		this.#opSince = performance.now();
		this.#opts.onAppend?.();
		return true;
	}
}
