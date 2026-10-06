import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { ApiError } from '@iris/api/client';
import type { Loadable } from '#lib/query.ts';
import { html } from '#lib/test/snippet.ts';
import Loaded from './Loaded.svelte';

/** A value the test drives: its state, and what a retry does. */
function value(init: Partial<Loadable>, onRefresh: (v: Loadable) => void = () => {}): Loadable {
	const v: Loadable = $state<Loadable>({
		loading: false,
		failed: false,
		error: undefined,
		stale: false,
		at: Date.now(),
		refresh: async (): Promise<void> => onRefresh(v),
		...init
	});
	return v;
}

const content = html('<p>Content</p>');

describe('Loaded', () => {
	it('the first load: a line, or skeletons of the tiles’ height; never a live region', async () => {
		await render(Loaded, { value: value({ loading: true }), skeletons: 2, children: content });
		expect(document.querySelectorAll('.skeleton')).toHaveLength(2);
		await expect.element(page.getByRole('status')).not.toBeInTheDocument();
	});

	it('nothing known and a failure: said as such, with the reason and a retry; the retry brings the content', async () => {
		const v = value({ failed: true, error: new Error('Iris is far away') }, (x) => {
			x.failed = false;
			x.error = undefined;
		});
		await render(Loaded, { value: v, children: content });
		await expect.element(page.getByText('This could not be loaded.')).toBeVisible();
		await expect.element(page.getByText('Iris is far away')).toBeVisible();
		await page.getByRole('button', { name: 'Try again' }).click();
		await expect.element(page.getByText('Content')).toBeVisible();
	});

	it('loaded and empty: why, and what to do', async () => {
		await render(Loaded, { value: value({}), empty: true, emptyText: 'Nothing here', emptyHint: 'Search for a title', children: content });
		await expect.element(page.getByText('Nothing here')).toBeVisible();
		await expect.element(page.getByText('Search for a title')).toBeVisible();
		await expect.element(page.getByText('Content')).not.toBeInTheDocument();
	});

	it('what the server says does not exist (404, `missing`): not found, nothing to retry', async () => {
		await render(Loaded, {
			value: value({ failed: true, error: new ApiError(404, 'not_found', '') }),
			missing: 'This collection is gone',
			children: content
		});
		await expect.element(page.getByText('This collection is gone')).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Try again' })).not.toBeInTheDocument();
	});

	it('an old value stays, said as such, with a way to ask again', async () => {
		await render(Loaded, { value: value({ stale: true, error: new Error('no answer') }), children: content });
		await expect.element(page.getByText('Content')).toBeVisible();
		await expect.element(page.getByText(/Last read/)).toBeVisible();
	});
});
