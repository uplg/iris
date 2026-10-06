import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import Poster from './Poster.svelte';

/** A tiny image that loads, as a data URL. */
function picture(): string {
	const c = document.createElement('canvas');
	c.width = c.height = 2;
	return c.toDataURL('image/png');
}

describe('Poster', () => {
	it('a poster that failed comes back when a working one replaces it', async () => {
		const { container, rerender } = await render(Poster, { src: '/missing-poster.jpg', title: 'Severance' });
		await expect.element(page.getByText('Severance')).toBeVisible();
		expect(container.querySelector('img')).toBeNull();
		await rerender({ src: picture() });
		await expect.poll(() => container.querySelector('img')).not.toBeNull();
	});
});
