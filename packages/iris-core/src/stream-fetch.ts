/**
 * The engines' own requests (mediabunny's byte ranges, the manifest, the seek hint) can't ride
 * the API client's `request()`, so they come through here: the `X-Iris-Client` header every
 * Iris request carries, and the session refreshed (single-flight, shared with the app) and the
 * request replayed once when the access cookie expired mid-playback. Without it a laptop waking
 * up answers its first range request 401, and the page demotes a healthy tier.
 */

import { IRIS_WEB_VERSION, refreshSessionForFetch } from '@iris/api/client';

type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export type IrisFetchDeps = {
	fetch: Fetch;
	/** Refreshes the session; true once it is alive again. */
	refresh: () => Promise<boolean>;
	/** `X-Iris-Client` value. */
	client: string;
};

const authFailed = (status: number) => status === 401 || status === 403;

export function createIrisFetch(deps: IrisFetchDeps): Fetch {
	return async (input, init) => {
		const send = () => {
			const headers = new Headers(init?.headers);
			headers.set('X-Iris-Client', deps.client);
			return deps.fetch(input, { ...init, headers });
		};
		const res = await send();
		if (!authFailed(res.status) || !(await deps.refresh())) return res;
		return send();
	};
}

/** For mediabunny: a 5xx (a redeploy, a source rotation) and an auth failure that survived the
 *  refresh become rejections, which its `getRetryDelay` ladder retries — `fetch` itself never
 *  rejects on a status, so they would otherwise fail the pipeline at once. */
export function createStreamFetch(base: Fetch): Fetch {
	return async (input, init) => {
		const res = await base(input, init);
		if (res.status >= 500 || authFailed(res.status)) throw new Error(`iris-stream-transient-${res.status}`);
		return res;
	};
}

export const irisFetch: Fetch = createIrisFetch({
	fetch: (input, init) => fetch(input, init),
	refresh: refreshSessionForFetch,
	client: `web/${IRIS_WEB_VERSION}`
});
