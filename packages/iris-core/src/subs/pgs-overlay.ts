/**
 * PGS bitmap subtitle overlay via `libpgs`.
 *
 * Cleaner integration than libass-wasm: `libpgs` is a proper ES module
 * with a worker shipped alongside. We pass it the host element + a
 * subtitle URL and it manages everything else (decode, scale, paint
 * sync via the supplied `<video>` element).
 *
 * Without a `<video>` (the player hands none: its canvas sits over every
 * engine, canvas ones included) a rAF loop renders at the engine's live
 * clock, only when it moved.
 */

const WORKER_URL = '/libpgs/libpgs.worker.js';

export type PgsOverlayOptions = {
	host: HTMLElement;
	subUrl: string;
	getCurrentTime: () => number;
	/** Native `<video>` element to bind to. When omitted, we run a rAF
	 *  loop that calls `renderAtTimestamp(getCurrentTime())`. */
	video?: HTMLVideoElement;
};

export type PgsOverlayHandle = {
	/** Hot-reload from a new URL without remounting the renderer.
	 *  Mirrors the ASS overlay so the parent's torrent-progress
	 *  watcher can swap both kinds uniformly. */
	setUrl: (url: string) => void;
	dispose: () => void;
};

export async function mountPgsOverlay(opts: PgsOverlayOptions): Promise<PgsOverlayHandle> {
	// libpgs is ~60 KB gzipped; deferring its load shaves it off the
	// initial bundle for users who never enable a PGS sub.
	const { PgsRenderer } = await import('libpgs');
	const canvas = document.createElement('canvas');
	canvas.className = 'pointer-events-none absolute inset-0 h-full w-full';
	opts.host.appendChild(canvas);

	const renderer = new PgsRenderer({
		canvas,
		workerUrl: WORKER_URL,
		video: opts.video,
		aspectRatio: 'contain'
	});
	renderer.loadFromUrl(opts.subUrl);

	let rafId: number | null = null;
	if (!opts.video) {
		let rendered = Number.NaN;
		const tick = () => {
			// read live, rendered only when it moved (paused: nothing to do)
			const t = opts.getCurrentTime();
			if (t !== rendered) {
				rendered = t;
				try {
					renderer.renderAtTimestamp(t);
				} catch {
					/* ignore */
				}
			}
			rafId = requestAnimationFrame(tick);
		};
		rafId = requestAnimationFrame(tick);
	}

	return {
		setUrl: (url: string) => {
			try {
				renderer.loadFromUrl(url);
			} catch (e) {
				console.warn('[iris-core:libpgs] loadFromUrl failed', e);
			}
		},
		dispose: () => {
			if (rafId !== null) cancelAnimationFrame(rafId);
			try {
				renderer.dispose();
			} catch {
				/* idempotent */
			}
			if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
		}
	};
}
