import { afterEach, describe, expect, it, vi } from 'vitest';
import { library, livetv, torrents } from './client';

type Call = { url: string; init?: RequestInit };

function script(...responses: Response[]) {
	const calls: Call[] = [];
	vi.stubGlobal(
		'fetch',
		vi.fn(async (url: string, init?: RequestInit) => {
			calls.push({ url, init });
			const next = responses.shift();
			if (!next) throw new Error(`unexpected call to ${url}`);
			return next;
		})
	);
	return calls;
}

const header = (c: Call) => (c.init?.headers as Record<string, string> | undefined)?.['X-Iris-Client'];

describe('calls outside JSON still go through the client', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('the live master: an expired access cookie refreshes once, then the headers are read', async () => {
		const calls = script(
			new Response(null, { status: 401 }),
			new Response(JSON.stringify({ id: 'u1' }), { status: 200, headers: { 'Content-Type': 'application/json' } }),
			new Response('#EXTM3U', { status: 200, headers: { 'x-iris-live-upstream': 'tuner', 'x-iris-live-sources': '3' } })
		);
		const headers = await livetv.masterHeaders('fr', 'tf1');
		expect(headers.get('x-iris-live-sources')).toBe('3');
		expect(calls.map((c) => c.url)).toEqual([
			'/api/livetv/fr/channels/tf1/master.m3u8',
			'/api/auth/refresh',
			'/api/livetv/fr/channels/tf1/master.m3u8'
		]);
		expect(calls.every((c) => header(c)?.startsWith('web/'))).toBe(true);
	});

	it('the live master: a refusal is an error', async () => {
		script(new Response(null, { status: 404 }));
		await expect(livetv.masterHeaders('fr', 'gone')).rejects.toMatchObject({ status: 404 });
	});

	it('the live master: a DRM-locked channel says so by its code', async () => {
		const body = JSON.stringify({ error: 'live_encrypted', message: "Encrypted by the broadcaster: it can't be played here." });
		script(new Response(body, { status: 409, headers: { 'Content-Type': 'application/json' } }));
		await expect(livetv.masterHeaders('ie', 'rteone')).rejects.toMatchObject({ status: 409, code: 'live_encrypted' });
	});

	it('a playback error report says who sends it and outlives the page', async () => {
		const calls = script(new Response(null, { status: 204 }));
		await torrents.reportPlaybackError('ih', 2, { tier: 'B', reason: 'decode', codec: 'hevc', browser: 'test' });
		expect(calls[0].url).toBe('/api/torrents/ih/files/2/playback-error');
		expect(calls[0].init).toMatchObject({ method: 'POST', keepalive: true });
		expect(header(calls[0])).toMatch(/^web\//);
	});

	it('a route parameter stays one path segment', async () => {
		const calls = script(new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } }));
		await library.collection('x/../../admin/users');
		expect(calls[0].url).toBe('/api/library/collections/x%2F..%2F..%2Fadmin%2Fusers');
	});
});
