import { describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import '../../styles/app.css';
import { controlAt } from '#lib/test/hit.ts';
import { release } from './fixtures.ts';
import LibraryMatches from './LibraryMatches.svelte';
import ReleaseCard from './ReleaseCard.svelte';
import ReleaseRow from './ReleaseRow.svelte';

vi.mock('$app/navigation', () => ({ goto: vi.fn(async () => {}) }));

describe('search cards open from anywhere on them', () => {
	it('a release in the grid: its poster is its one link', async () => {
		const { container } = await render(ReleaseCard, { r: release() });
		const link = page.getByRole('link', { name: 'Severance' });
		expect(container.querySelectorAll('a')).toHaveLength(1);
		expect(controlAt(container.querySelector('.art'))).toBe(link.element());
		expect(controlAt(container.querySelector('.release'))).toBe(link.element());
	});

	it('a release in the list: its poster opens it, its grab button stays its own', async () => {
		const { container } = await render(ReleaseRow, { r: release() });
		const link = page.getByRole('link', { name: /Severance/ });
		expect(controlAt(container.querySelector('.mini'))).toBe(link.element());
		const grab = page.getByRole('button', { name: 'Grab and play' });
		expect(controlAt(grab.element())).toBe(grab.element());
	});

	it('a library match: its poster opens the title, its play link stays its own', async () => {
		const { container } = await render(LibraryMatches, {
			matches: [
				{
					collection_id: 'c1',
					display_title: 'Severance',
					episode_count: 19,
					episode_infohash: 'aa',
					episode_file_idx: 3,
					episode_season: 2,
					episode_number: 4,
					is_anime: false,
					kind: 'tv',
					torrent_count: 2
				}
			]
		});
		const link = page.getByRole('link', { name: 'Severance', exact: true });
		expect(controlAt(container.querySelector('.mini'))).toBe(link.element());
		const play = page.getByRole('link', { name: 'Play S2:E4: Severance' });
		await expect.element(play).toHaveAttribute('href', '/watch/aa/3');
		expect(controlAt(play.element())).toBe(play.element());
	});
});
