// MediaSource lifecycle helpers shared by the MSE engines.

/** Resolves on `sourceopen`, rejects if the MediaSource errors first. */
export function openMediaSource(ms: MediaSource, label: string): Promise<void> {
	return new Promise<void>((resolve, reject) => {
		const onOpen = () => {
			ms.removeEventListener('sourceopen', onOpen);
			ms.removeEventListener('error', onErr);
			resolve();
		};
		const onErr = () => {
			ms.removeEventListener('sourceopen', onOpen);
			ms.removeEventListener('error', onErr);
			reject(new Error(`${label}: MediaSource errored before opening`));
		};
		ms.addEventListener('sourceopen', onOpen);
		ms.addEventListener('error', onErr);
	});
}

/** Resolves once the SourceBuffer is idle: its operation ended, failed or was aborted. */
export function waitForIdle(sb: SourceBuffer | null): Promise<void> {
	return new Promise<void>((resolve) => {
		if (!sb || !sb.updating) {
			resolve();
			return;
		}
		const done = () => {
			for (const e of ['updateend', 'error', 'abort']) sb.removeEventListener(e, done);
			resolve();
		};
		for (const e of ['updateend', 'error', 'abort']) sb.addEventListener(e, done);
	});
}

/** Ends the stream if the MediaSource is still open. */
export function endStream(ms: MediaSource | null): void {
	try {
		if (ms && ms.readyState === 'open') ms.endOfStream();
	} catch {
		/* idempotent */
	}
}

/** Tears an engine's `<video>` down for good. `load()` after dropping `src` is the
 *  spec-defined reset: it aborts the resource and returns the element to NETWORK_EMPTY, which
 *  releases the MediaSource and the platform decoder now rather than at GC (Firefox's small
 *  VideoToolbox pool otherwise starves the next engine: `kVTVideoDecoderBadDataErr` on its
 *  first append). The element leaves the DOM too: the next engine may already be mounted in
 *  the same container. */
export function releaseVideo(video: HTMLVideoElement, objectUrl?: string | null): void {
	try {
		video.pause();
	} catch {
		/* idempotent */
	}
	try {
		video.removeAttribute('src');
		video.load();
	} catch {
		/* idempotent */
	}
	if (objectUrl) URL.revokeObjectURL(objectUrl);
	video.remove();
}
