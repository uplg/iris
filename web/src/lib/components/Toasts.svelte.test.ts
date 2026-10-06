import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { flushSync } from 'svelte';
import { ui } from '#lib/ui.svelte.ts';
import Toasts from './Toasts.svelte';

describe('Toasts', () => {
	afterEach(() => {
		for (const t of ui.toasts.slice()) ui.dismiss(t.id);
	});

	it('shows each message, and closes it from its button', async () => {
		await render(Toasts);
		ui.toast('Saved');
		ui.toast('Iris did not answer', { warn: true });
		await expect.element(page.getByText('Saved')).toBeVisible();
		await expect.element(page.getByText('Iris did not answer')).toBeVisible();
		await page.getByRole('button', { name: 'Close' }).first().click();
		await expect.element(page.getByText('Saved')).not.toBeInTheDocument();
		await expect.element(page.getByText('Iris did not answer')).toBeVisible();
	});

	it('lets a message go when its countdown ends; a failure has none and stays', async () => {
		const { container } = await render(Toasts);
		ui.toast('Saved');
		ui.toast('Iris did not answer', { warn: true });
		flushSync();
		const [saved, failure] = container.querySelectorAll<HTMLElement>('.toast');
		expect(saved.classList.contains('countdown')).toBe(true);
		expect(failure.classList.contains('countdown')).toBe(false);
		saved.dispatchEvent(new AnimationEvent('animationend', { animationName: 'toast-countdown' }));
		flushSync();
		expect(container.textContent).not.toContain('Saved');
		expect(container.textContent).toContain('Iris did not answer');
	});

	it('gives a toast with an action the longer countdown', async () => {
		const { container } = await render(Toasts);
		ui.toast('Removed from the row', { action: { label: 'Undo', run: () => {} } });
		flushSync();
		expect(container.querySelector('.toast.countdown.long')).not.toBeNull();
	});

	it('an action: its button does it, the toast leaves, the focus goes where the action says', async () => {
		await render(Toasts);
		const back = document.createElement('button');
		back.textContent = 'Add';
		document.body.append(back);
		const run = vi.fn();
		const id = ui.toast('Removed from the row', { action: { label: 'Undo', run, back: () => back } });
		const restore = page.getByRole('button', { name: 'Undo' });
		await expect.element(restore).toHaveAttribute('id', `toast-action-${id}`);
		(restore.element() as HTMLElement).focus();
		await restore.click();
		expect(run).toHaveBeenCalledOnce();
		await expect.element(page.getByText('Removed from the row')).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Add' })).toHaveFocus();
		back.remove();
	});
});
