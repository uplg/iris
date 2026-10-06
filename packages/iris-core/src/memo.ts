/** `load` run once and its answer shared, until it fails: a failed load (a network blip) is
 * asked again by the next caller instead of failing every later playback until a reload. */
export function onceUntilFailure<T>(load: () => Promise<T>): () => Promise<T> {
	let pending: Promise<T> | null = null;
	return () => {
		pending ??= load().catch((e: unknown) => {
			pending = null;
			throw e;
		});
		return pending;
	};
}
