/**
 * The muxer's chunks on their way into one SourceBuffer. `appendBuffer` takes one chunk at a
 * time, so they queue here and `pump` hands over the next one whenever the buffer is idle; the
 * engine pumps from its own events (`updateend`, `timeupdate`, a stall). A QuotaExceededError
 * keeps the chunk at the head for a later pump and lets the engine free room: no retry loop,
 * the next event retries.
 */

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
	#opts: AppendQueueOptions;
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
		if (this.#opts.proxied) {
			for (const e of ['updateend', 'error', 'abort']) sb.addEventListener(e, this.#settle);
		}
	}

	detach(): void {
		if (this.#sb && this.#opts.proxied) {
			for (const e of ['updateend', 'error', 'abort']) this.#sb.removeEventListener(e, this.#settle);
		}
		this.#sb = null;
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
		this.#opts.onAppend?.();
		return true;
	}
}
