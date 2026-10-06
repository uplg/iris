import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { html } from '#lib/test/snippet.ts';
import Shelf from './Shelf.svelte';

const CARD = 'style="flex: none; width: 200px; height: 20px"';

describe('Shelf', () => {
	it('cards arriving later make the forward pager usable', async () => {
		const { container } = await render(Shelf, { title: 'Trending', children: html(`<li ${CARD}>One</li>`) });
		container.style.width = '320px';
		const forward = page.getByRole('button', { name: 'Scroll Trending forward' });
		await expect.element(forward).toHaveAttribute('aria-disabled', 'true');
		const rail = container.querySelector('ul')!;
		// an overlay scrollbar (macOS): the rail's own box does not change as it overflows
		rail.style.scrollbarWidth = 'none';
		const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
		await frame();
		await frame();
		for (const name of ['Two', 'Three', 'Four']) rail.insertAdjacentHTML('beforeend', `<li ${CARD}>${name}</li>`);
		await expect.element(forward).toHaveAttribute('aria-disabled', 'false');
	});
});
