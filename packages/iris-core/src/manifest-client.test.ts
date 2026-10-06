import { describe, expect, it } from 'vitest';
import { ManifestNotReadyError, readManifestResponse } from './manifest-client';

const json = (status: number, body: unknown) =>
	new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

describe('readManifestResponse', () => {
	it('reads the manifest of a 200', async () => {
		await expect(readManifestResponse(json(200, { infohash: 'abc' }))).resolves.toEqual({ infohash: 'abc' });
	});

	it('says « not ready » for a 400 while the file is not on disk', async () => {
		await expect(readManifestResponse(json(400, { message: 'file not yet on disk' }))).rejects.toBeInstanceOf(ManifestNotReadyError);
	});

	it("keeps the server's message for any other 400", async () => {
		const err = await readManifestResponse(json(400, { message: 'unsupported container' })).catch((e: unknown) => e);
		expect(err).not.toBeInstanceOf(ManifestNotReadyError);
		expect((err as Error).message).toBe('unsupported container');
	});

	it('names the status when the server says nothing', async () => {
		await expect(readManifestResponse(new Response('', { status: 502 }))).rejects.toThrow('manifest fetch failed (502)');
	});
});
