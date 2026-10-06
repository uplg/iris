// The engines' `UrlSource`, apart from `stream-fetch.ts` so the watch page's `irisFetch` and
// manifest read don't pull mediabunny in before a tier is picked.

import { UrlSource } from 'mediabunny';
import { createStreamFetch, irisFetch } from './stream-fetch';

const streamFetch = createStreamFetch(irisFetch);

export type StreamSourceOptions = {
	/** Mediabunny's read cache (its default, 64 MiB, sits on top of the SourceBuffer). */
	cacheBytes: number;
	/** Retries before the stream is declared broken, with capped exponential backoff. */
	attempts: number;
	maxDelayS: number;
};

/** The one `UrlSource` every engine reads the server through. */
export function irisUrlSource(url: string, opts: StreamSourceOptions): UrlSource {
	return new UrlSource(url, {
		fetchFn: streamFetch,
		getRetryDelay: (attempts) => (attempts >= opts.attempts ? null : Math.min(opts.maxDelayS, 0.5 * 2 ** attempts)),
		maxCacheSize: opts.cacheBytes
	});
}

/** VOD `/stream`: capped backoff (~0.5, 1, 2, 4, 8, 8… s) over ~70 s, a typical
 *  deploy/restart window, then give up so a broken stream still surfaces. */
export const VOD_RETRY = { attempts: 12, maxDelayS: 8 } as const;
