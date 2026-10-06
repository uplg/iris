import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import PosterCard from './PosterCard.svelte';

const RELEASE_NAME = 'Puis.Je.Vous.Aider.S01.VOSTFR.1080p.WEBRip.AAC.2.0.x264-NoTag';

describe('PosterCard', () => {
	it.each(['poster', 'still'] as const)('a title that is one long word keeps the %s art at the card width', async (shape) => {
		const { container } = await render(PosterCard, { href: '/title/1', title: RELEASE_NAME, art: null, shape });
		await expect.element(page.getByRole('link', { name: RELEASE_NAME })).toBeVisible();
		const card = container.querySelector('li')!.getBoundingClientRect();
		const art = container.querySelector('.art')!.getBoundingClientRect();
		const name = container.querySelector('.name')!.getBoundingClientRect();
		expect(card.width).toBeGreaterThan(0);
		expect(art.width).toBeCloseTo(card.width, 0);
		expect(name.right).toBeLessThanOrEqual(card.right + 0.5);
	});
});
