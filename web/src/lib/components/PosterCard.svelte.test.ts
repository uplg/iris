import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import '../../styles/app.css';
import { controlAt } from '#lib/test/hit.ts';
import MenuCard from '#lib/home/test/MenuCard.svelte';
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

	it('opens from its art through its one link; its menu still opens above it', async () => {
		const run = vi.fn();
		const { container } = await render(MenuCard, { run });
		const link = page.getByRole('link', { name: 'Andor' });
		await expect.element(link).toHaveAttribute('href', '/collection/c1');
		expect(container.querySelectorAll('a')).toHaveLength(1);
		expect(controlAt(container.querySelector('.art'))).toBe(link.element());
		expect(controlAt(container.querySelector('.meta'))).toBe(link.element());

		const menu = page.getByRole('button', { name: 'More for Andor' });
		expect(controlAt(menu.element())).toBe(menu.element());
		await menu.click();
		await page.getByRole('menuitem', { name: 'Remove from your watchlist' }).click();
		expect(run).toHaveBeenCalledOnce();
	});

	it('the focus ring goes around the art, the pointer outlines it', async () => {
		const { container } = await render(PosterCard, { href: '/title/1', title: 'Andor', art: null });
		const card = container.querySelector('li')!;
		const art = container.querySelector<HTMLElement>('.art')!;
		const link = page.getByRole('link', { name: 'Andor' });
		await userEvent.hover(link);
		await expect.poll(() => getComputedStyle(art).outlineWidth).toBe('2px');
		await userEvent.unhover(link);
		await userEvent.tab();
		await expect.poll(() => getComputedStyle(art).outlineWidth).toBe('3px');
		expect(getComputedStyle(art).outlineStyle).toBe('solid');
		expect(getComputedStyle(card).outlineStyle).toBe('none');
	});
});
