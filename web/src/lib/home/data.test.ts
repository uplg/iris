import { describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { downloadsByCollection, friendRows, noteDue, rightNow, secondsLeft } from './data.ts';

describe('home words', () => {
	it('"Right now": only what is downloading, in words', () => {
		expect(
			rightNow({
				downloading: 2,
				downloading_pct: 63.6,
				downloading_eta_seconds: 1380,
				new_episodes: 3,
				seeding: 18,
				disk: { free_bytes: 412 * 1024 ** 3, total_bytes: 2000 * 1024 ** 3 }
			})
		).toEqual(['2 downloads · 64% · 23 min left']);
		expect(rightNow({ downloading: 1, downloading_pct: 10, new_episodes: 1, seeding: 1 })).toEqual(['1 download · 10%']);
		expect(rightNow({ downloading: 0, downloading_pct: 0, new_episodes: 0, seeding: 0 })).toEqual([]);
	});

	describe('friends, a line each', () => {
		const at = '2026-10-05T20:00:00Z';
		const muriel = { friend_id: 'f', friend_name: 'Muriel', at };
		const news = (title: string, action: 'watched' | 'added' = 'watched') => ({ ...muriel, action, title });

		it('a friend and their latest three, a verb repeated once', () => {
			const [row, ...others] = friendRows({
				live: [{ ...muriel, action: 'watching', title: 'Severance', season: 2, episode: 4 }],
				recent: [news('Dune'), news('Alien'), news('Arcane', 'added')]
			});
			expect(others).toEqual([]);
			expect(row.at).toBeNull();
			expect(row.items.map((i) => [i.verb, i.title])).toEqual([
				['is watching', 'Severance · S2:E4'],
				['watched', 'Dune'],
				[null, 'Alien']
			]);
		});

		it('three friends at most, when said from their latest', () => {
			const someone = (id: string) => ({ ...news('Dune'), friend_id: id, friend_name: id });
			const rows = friendRows({ live: [], recent: ['a', 'b', 'a', 'c', 'd'].map(someone) });
			expect(rows.map((r) => r.name)).toEqual(['a', 'b', 'c']);
			expect(rows[0].at).toBe(at);
		});
	});

	it('the update sentence: after an update only, and only when the release has one', () => {
		const release = { version: '2.0.0', note: 'Two more trackers.' };
		expect(noteDue(release, '1.9.0')).toBe('Two more trackers.');
		expect(noteDue(release, '2.0.0')).toBeUndefined();
		expect(noteDue(release, undefined)).toBeUndefined();
		expect(noteDue({ version: '2.0.0', note: null }, '1.9.0')).toBeUndefined();
	});

	it('downloads per collection, by bytes; finished ones and loose torrents left out', () => {
		const t = (collection_id: string | null, done: number, total: number, finished = false) =>
			({ collection_id, progress_bytes: done, total_size_bytes: total, finished }) as TorrentView;
		const m = downloadsByCollection([t('a', 1, 4), t('a', 3, 4), t('b', 5, 5, true), t(null, 1, 2)]);
		expect([...m]).toEqual([['a', 50]]);
	});

	it('time left only when the length is known', () => {
		expect(secondsLeft({ position_seconds: 1930, duration_seconds: 3300 })).toBe(1370);
		expect(secondsLeft({ position_seconds: 10, duration_seconds: null })).toBeNull();
	});
});
