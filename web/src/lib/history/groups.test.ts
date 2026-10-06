import { describe, expect, it } from 'vitest';
import type { HistoryItem } from '@iris/api/client';
import { canRestore, groupHistory } from './groups.ts';

const item = (over: Partial<HistoryItem>): HistoryItem => ({
	infohash: 'aa',
	file_idx: 0,
	torrent_name: 'Release.Name.2024.1080p',
	completed: false,
	deleted: false,
	last_watched_at: '2026-10-06T10:00:00Z',
	position_seconds: 0,
	tmdb_verified: true,
	...over
});

describe('groupHistory', () => {
	it('a series under its title, in the order of its latest watch; a film on its own line', () => {
		const groups = groupHistory([
			item({ infohash: 's1', collection_id: 'c1', collection_title: 'Severance', season: 2, episode: 4 }),
			item({ infohash: 'm1', torrent_name: 'Dune.2021.mkv' }),
			item({ infohash: 's2', collection_id: 'c1', collection_title: 'Severance', season: 2, episode: 3 })
		]);
		expect(groups.map((g) => [g.title, g.items.length, g.solo])).toEqual([
			['Severance', 2, false],
			['Dune (2021)', 1, true]
		]);
	});

	it('a title whose files are all gone stays listed, marked as such', () => {
		const [g] = groupHistory([
			item({ collection_id: 'c1', collection_title: 'Lost', season: 1, episode: 1, deleted: true }),
			item({ infohash: 'bb', collection_id: 'c1', collection_title: 'Lost', season: 1, episode: 2, deleted: true })
		]);
		expect(g.ghost).toBe(true);
		expect(groupHistory([item({ deleted: true }), item({ infohash: 'x' })]).every((x) => x.solo)).toBe(true);
	});

	it('can be downloaded again only when gone and its release is known', () => {
		expect(canRestore(item({ deleted: true, source_provider: 'c411', source_external_id: '42' }))).toBe(true);
		expect(canRestore(item({ deleted: true }))).toBe(false);
		expect(canRestore(item({ source_provider: 'c411', source_external_id: '42' }))).toBe(false);
	});
});
