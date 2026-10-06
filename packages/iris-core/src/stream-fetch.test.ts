import { describe, expect, it, vi } from 'vitest';
import { createIrisFetch, createStreamFetch } from './stream-fetch';

const response = (status: number) => new Response(null, { status });

function setup(statuses: number[], refreshed = true) {
	const queue = [...statuses];
	const fetch = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => response(queue.shift() ?? 200));
	const refresh = vi.fn(async () => refreshed);
	return { fetch, refresh, irisFetch: createIrisFetch({ fetch, refresh, client: 'web/9.9.9' }) };
}

describe('createIrisFetch', () => {
	it('stamps the client header and keeps the caller own headers', async () => {
		const { fetch, irisFetch } = setup([206]);
		const res = await irisFetch('/api/torrents/x/files/0/stream', { headers: { Range: 'bytes=0-99' } });
		expect(res.status).toBe(206);
		const headers = new Headers(fetch.mock.calls[0]?.[1]?.headers);
		expect(headers.get('X-Iris-Client')).toBe('web/9.9.9');
		expect(headers.get('Range')).toBe('bytes=0-99');
	});

	it('refreshes the session once on a 401 and replays the request', async () => {
		const { fetch, refresh, irisFetch } = setup([401, 206]);
		const res = await irisFetch('/stream', { headers: { Range: 'bytes=0-1' } });
		expect(res.status).toBe(206);
		expect(refresh).toHaveBeenCalledTimes(1);
		expect(fetch).toHaveBeenCalledTimes(2);
		expect(new Headers(fetch.mock.calls[1]?.[1]?.headers).get('Range')).toBe('bytes=0-1');
	});

	it('gives the 401 back when the session could not be refreshed', async () => {
		const { fetch, irisFetch } = setup([401], false);
		expect((await irisFetch('/stream')).status).toBe(401);
		expect(fetch).toHaveBeenCalledTimes(1);
	});

	it('leaves other statuses alone', async () => {
		const { refresh, irisFetch } = setup([404]);
		expect((await irisFetch('/stream')).status).toBe(404);
		expect(refresh).not.toHaveBeenCalled();
	});
});

describe('createStreamFetch', () => {
	it('turns a 5xx and a lasting auth failure into a retryable rejection', async () => {
		for (const status of [502, 401, 403]) {
			const fetch = createStreamFetch(async () => response(status));
			await expect(fetch('/stream')).rejects.toThrow(`iris-stream-transient-${status}`);
		}
	});

	it('hands every other answer to mediabunny', async () => {
		const fetch = createStreamFetch(async () => response(416));
		expect((await fetch('/stream')).status).toBe(416);
	});
});
