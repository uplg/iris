import { describe, expect, it, vi } from 'vitest';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';
import { stubApi } from '#lib/test/api.ts';
import { Grab } from './grab.svelte.ts';

const preview = {
	name: 'Severance.S01E01.1080p',
	streamable: true,
	total_size_bytes: 2_000_000_000,
	files: [{ index: 0, path: 'Severance.S01E01.1080p.mkv', size_bytes: 2_000_000_000 }]
};

describe('a grab', () => {
	it('reads the library again, and marks the results that said « not in library » old', async () => {
		stubApi({
			'POST /torrents/preview': preview,
			'POST /torrents': { snapshot: { infohash: 'ih1' } }
		});
		queryClient.setQueryData(KEYS.torrents, { view: 'torrents', items: [] });
		queryClient.setQueryData([...KEYS.search, 'severance'], { pages: [], pageParams: [] });
		const go = vi.fn();
		await new Grab(go).run({ provider: 'p', id: 'x' });
		expect(go).toHaveBeenCalledWith('/watch/ih1/0');
		expect(queryClient.getQueryState(KEYS.torrents)?.isInvalidated).toBe(true);
		expect(queryClient.getQueryState([...KEYS.search, 'severance'])?.isInvalidated).toBe(true);
	});
});
