import { afterEach, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import StageTopBar from './StageTopBar.svelte';

afterEach(() => document.body.replaceChildren());

it('a film steps back through its handler instead of following the link', async () => {
	const onclick = vi.fn((e: MouseEvent) => e.preventDefault());
	await render(StageTopBar, { props: { back: { href: '/library', label: 'Back', onclick }, title: 'Avatar: Fire and Ash' } });
	const link = page.getByRole('link', { name: 'Back', exact: true });
	await expect.element(link).toHaveAttribute('href', '/library');
	await link.click();
	expect(onclick).toHaveBeenCalledOnce();
	expect(location.pathname).not.toBe('/library');
});

it('a series names its page', async () => {
	await render(StageTopBar, { props: { back: { href: '/collection/c1', label: 'Back to Silo' }, title: 'S1:E1' } });
	await expect.element(page.getByRole('link', { name: 'Back to Silo' })).toHaveAttribute('href', '/collection/c1');
});
