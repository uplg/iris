/**
 * Where the feed loops park: until the other track catches up, until playback makes room, until
 * an append lands. Woken by events (`notify` on progress, `timeupdate`…), never by a timer, so
 * a gate can't deadlock playback. A restart or a dispose `flush`es it: every parked loop wakes,
 * sees its generation is stale and exits, instead of waking much later and writing its state
 * over the live pipeline's.
 */

type Waiter = { ready: () => boolean; resolve: () => void; label: string | null };

export class FeedGate {
	#waiters = new Set<Waiter>();
	/** Why a loop is parked right now (the most recent one), for the diagnostics. */
	parked: string | null = null;

	/** Resolves once `ready()` holds; `label` says why it parks meanwhile. */
	wait(ready: () => boolean, label?: () => string): Promise<void> {
		if (ready()) return Promise.resolve();
		return new Promise<void>((resolve) => {
			const w: Waiter = { ready, resolve, label: label?.() ?? null };
			this.#waiters.add(w);
			if (w.label) this.parked = w.label;
		});
	}

	/** Re-checks every parked loop; the ready ones go. */
	notify(): void {
		for (const w of this.#waiters) {
			if (w.ready()) this.#release(w);
		}
	}

	/** Wakes every parked loop, ready or not. */
	flush(): void {
		for (const w of this.#waiters) this.#release(w);
	}

	get size(): number {
		return this.#waiters.size;
	}

	#release(w: Waiter): void {
		this.#waiters.delete(w);
		if (this.parked !== null && this.parked === w.label) this.parked = null;
		w.resolve();
	}
}
