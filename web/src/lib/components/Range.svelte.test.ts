import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { flushSync } from 'svelte';
import { ui } from '#lib/ui.svelte.ts';
import Range from './Range.svelte';

const valueText = (v: number) => `Volume ${v} %`;
const base = { label: 'Volume', value: 50, valueText };

const thumb = () => page.getByRole('slider').element() as HTMLElement;
const said = () => thumb().getAttribute('aria-valuetext');

/** A pointer gesture on the band: down at the first position (0–1 of the track), moves, release. */
function pointer(type: string, at?: number) {
	// the band around the thumb: the 44 px target (WCAG 2.5.7)
	const band = thumb().parentElement!;
	const { left, width, top, height } = band.getBoundingClientRect();
	const init = { bubbles: true, button: 0, clientX: left + (at ?? 0) * width, clientY: top + height / 2, pointerId: 1 };
	(type === 'pointerdown' ? band : document).dispatchEvent(new PointerEvent(type, init));
	flushSync();
}
const press = (at: number) => pointer('pointerdown', at);
const move = (at: number) => pointer('pointermove', at);
const release = () => pointer('pointerup');

describe('Range', () => {
	it('is a slider named by its label, its value said in words', async () => {
		await render(Range, base);
		const slider = page.getByRole('slider', { name: 'Volume' });
		await expect.element(slider).toHaveAttribute('aria-valuenow', '50');
		await expect.element(slider).toHaveAttribute('aria-valuetext', 'Volume 50 %');
		await expect.element(page.getByText('Volume 50 %')).toBeVisible();
	});

	it('can keep its label for readers only, and say why it cannot act (still focusable)', async () => {
		const why = document.createElement('p');
		why.id = 'why';
		why.textContent = 'Unavailable';
		document.body.append(why);
		await render(Range, { ...base, hideLabel: true, reason: 'why' });
		const slider = page.getByRole('slider', { name: 'Volume' });
		await expect.element(slider).toHaveAttribute('aria-disabled', 'true');
		await expect.element(slider).toHaveAccessibleDescription('Unavailable');
		await expect.element(slider).toHaveAttribute('tabindex', '0');
		why.remove();
		// the label stays for readers only
		expect(page.getByText('Volume', { exact: true }).element().closest('.sr-only')).not.toBeNull();
	});

	describe('a form value', () => {
		it('commits on release only, not while dragging', async () => {
			const oncommit = vi.fn();
			await render(Range, { ...base, oncommit });
			press(0.2);
			move(0.3);
			move(0.4);
			expect(oncommit).not.toHaveBeenCalled();
			expect(said()).toBe('Volume 40 %');
			release();
			expect(oncommit).toHaveBeenCalledExactlyOnceWith(40);
		});

		it('commits on each key press', async () => {
			const oncommit = vi.fn();
			await render(Range, { ...base, oncommit });
			thumb().focus();
			await userEvent.keyboard('{ArrowRight}{ArrowRight}');
			expect(oncommit.mock.calls).toEqual([[51], [52]]);
			expect(said()).toBe('Volume 52 %');
		});

		it('moves a page of steps with Page Up / Page Down, within its bounds', async () => {
			const oncommit = vi.fn();
			await render(Range, { ...base, value: 95, step: 5, page: 2, oncommit });
			thumb().focus();
			await userEvent.keyboard('{PageDown}');
			expect(oncommit).toHaveBeenLastCalledWith(85);
			expect(said()).toBe('Volume 85 %');
			await userEvent.keyboard('{PageUp}{PageUp}');
			expect(oncommit).toHaveBeenLastCalledWith(100);
			await userEvent.keyboard('{Home}');
			await userEvent.keyboard('{PageDown}');
			expect(oncommit).toHaveBeenLastCalledWith(0);
		});

		it('Page keys do nothing when it cannot act', async () => {
			const oncommit = vi.fn();
			await render(Range, { ...base, oncommit, reason: 'why' });
			thumb().dispatchEvent(new KeyboardEvent('keydown', { key: 'PageUp', bubbles: true }));
			flushSync();
			expect(oncommit).not.toHaveBeenCalled();
			expect(said()).toBe('Volume 50 %');
		});
	});

	describe('a call that may fail', () => {
		it('sends on release only, never a timer between', async () => {
			const send = vi.fn(async (_v: number) => {});
			await render(Range, { ...base, send });
			press(0.2);
			move(0.3);
			expect(send).not.toHaveBeenCalled();
			release();
			expect(send).toHaveBeenCalledExactlyOnceWith(30);
		});

		it('live: sends every move at once, and the release', async () => {
			const send = vi.fn(async (_v: number) => {});
			await render(Range, { ...base, send, live: true });
			press(0.1);
			move(0.2);
			move(0.35);
			release();
			expect(send.mock.calls.map(([v]) => v)).toEqual([10, 20, 35, 35]);
		});

		it('Page Up sends a page of steps at once', async () => {
			const send = vi.fn(async (_v: number) => {});
			await render(Range, { ...base, send, page: 10 });
			thumb().dispatchEvent(new KeyboardEvent('keydown', { key: 'PageUp', bubbles: true, cancelable: true }));
			flushSync();
			expect(send).toHaveBeenCalledExactlyOnceWith(60);
		});

		it('a failed send is said, and the thumb goes back to the value', async () => {
			const fail = vi.spyOn(ui, 'fail').mockImplementation(() => {});
			const error = new Error('No answer');
			const send = vi.fn(async (_v: number) => {
				throw error;
			});
			await render(Range, { ...base, send });
			press(0.9);
			release();
			await expect.poll(() => fail.mock.calls.length).toBe(1);
			expect(fail).toHaveBeenCalledWith(error);
			flushSync();
			expect(said()).toBe('Volume 50 %');
			fail.mockRestore();
		});
	});
});
