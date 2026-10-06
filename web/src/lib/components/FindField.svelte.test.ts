import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import BackLink from './BackLink.svelte';
import FindField from './FindField.svelte';

describe('FindField', () => {
	it('a labelled search with its hint; Clear empties it and gives the focus back', async () => {
		await render(FindField, { id: 'find', label: 'Find a release', value: '', hint: 'By title or hash' });
		const field = page.getByRole('searchbox', { name: 'Find a release' });
		await expect.element(field).toHaveAccessibleDescription('By title or hash');
		await expect.element(page.getByRole('button', { name: 'Clear' })).not.toBeInTheDocument();
		await field.fill('dune');
		await page.getByRole('button', { name: 'Clear' }).click();
		await expect.element(field).toHaveValue('');
		await expect.element(field).toHaveFocus();
	});
});

describe('BackLink', () => {
	it('names where it leads', async () => {
		await render(BackLink, { href: '/admin', label: 'Admin' });
		await expect.element(page.getByRole('link', { name: 'Admin' })).toHaveAttribute('href', '/admin');
	});
});
