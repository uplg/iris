import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { text } from '#lib/test/snippet.ts';
import PageHead from './PageHead.svelte';

describe('PageHead', () => {
	it('titles the window « X · Iris » and the page with one h1', async () => {
		await render(PageHead, { title: 'Library' });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
		expect(document.querySelectorAll('h1')).toHaveLength(1);
		await expect.poll(() => document.title).toBe(`Library · ${'Iris'}`);
		await expect.element(page.getByRole('link', { name: 'Home' })).not.toBeInTheDocument();
	});

	it('the home page is titled « Iris » alone', async () => {
		await render(PageHead, { title: 'Iris' });
		await expect.poll(() => document.title).toBe('Iris');
	});

	it('a title known without being read (the dashboard) stays for screen readers, not drawn', async () => {
		await render(PageHead, { title: 'Home', hidden: true });
		const h1 = page.getByRole('heading', { level: 1, name: 'Home' });
		await expect.element(h1).toBeInTheDocument();
		await expect.element(h1).toHaveClass('sr-only');
		await expect.poll(() => document.title).toBe(`${'Home'} · ${'Iris'}`);
	});

	it('the h1 can take the focus after a navigation', async () => {
		await render(PageHead, { title: 'Library' });
		const h1 = document.querySelector('h1')!;
		h1.focus();
		expect(document.activeElement).toBe(h1);
	});

	it('a device page has a way back home, a line under and actions', async () => {
		await render(PageHead, { title: 'Lampe', back: true, sub: text('Allumée, 80 %'), end: text('Réglages') });
		await expect.element(page.getByRole('link', { name: 'Home' })).toHaveAttribute('href', '/');
		await expect.element(page.getByText('Allumée, 80 %')).toBeVisible();
		await expect.element(page.getByText('Réglages')).toBeVisible();
	});
});
