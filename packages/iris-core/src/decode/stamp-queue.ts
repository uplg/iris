/**
 * The timeline stamps of the frames handed to a `VideoEncoder`, in feed order, matched back to
 * its output chunks. A chunk echoes its input frame's timestamp (the decoder's µs value, which
 * we never rewrite), so the match is by that key: a realtime-mode encoder may DROP a frame under
 * load, and a plain FIFO would then shift every later chunk onto an earlier frame's stamp — a
 * video drift against audio that only grows. The FIFO order remains the fallback for an encoder
 * that doesn't echo.
 */

type Stamp = { key: number; rel: number };

export class StampQueue {
	#stamps: Stamp[] = [];
	/** Frames the encoder dropped (their stamps were skipped over). */
	dropped = 0;
	/** Chunks whose timestamp matched no frame we fed (FIFO fallback used). */
	unmatched = 0;

	get size(): number {
		return this.#stamps.length;
	}

	/** A frame went into the encoder: its input timestamp and its timeline position. */
	push(key: number, rel: number): void {
		this.#stamps.push({ key, rel });
	}

	/** The timeline position of the chunk the encoder just produced; null when nothing is
	 *  pending at all. */
	take(chunkTimestamp: number): number | null {
		const i = this.#stamps.findIndex((s) => s.key === chunkTimestamp);
		if (i < 0) {
			const head = this.#stamps.shift();
			if (!head) return null;
			this.unmatched += 1;
			return head.rel;
		}
		this.dropped += i;
		const [hit] = this.#stamps.splice(0, i + 1).slice(-1);
		return hit!.rel;
	}

	clear(): void {
		this.#stamps.length = 0;
	}
}
