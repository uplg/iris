import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { flushSync } from 'svelte';
import IrisChrome, { HIDE_AFTER_MS } from './IrisChrome.svelte';
import { MediaState } from './media-state.svelte.ts';
import { fakeEngine, testManifest } from './testing.ts';

async function setup(init: Parameters<typeof fakeEngine>[0] = {}, extra: Record<string, unknown> = {}) {
	const stage = document.createElement('div');
	stage.style.position = 'relative';
	stage.style.width = '960px';
	stage.style.height = '540px';
	document.body.append(stage);
	const engine = fakeEngine(init);
	const media = new MediaState(3300);
	media.read(engine);
	const onAudioPick = vi.fn();
	const onSubtitleChange = vi.fn();
	const onVisibleChange = vi.fn();
	const onVolumeChange = vi.fn();
	const screen = await render(IrisChrome, {
		target: stage,
		props: {
			handle: engine,
			media,
			manifest: testManifest(),
			activeSubtitle: null,
			onSubtitleChange,
			activeAudioIndex: 0,
			onAudioPick,
			fullscreenTarget: stage,
			pip: { supported: true, active: false, toggle: vi.fn(async () => undefined) },
			onVisibleChange,
			onVolumeChange,
			...extra
		}
	});
	const key = (k: string, target: Element = stage) => {
		target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
		flushSync();
	};
	const shown = () => stage.querySelector('.chrome')!.classList.contains('shown');
	return { stage, engine, media, onAudioPick, onSubtitleChange, onVisibleChange, onVolumeChange, key, shown, screen };
}

afterEach(() => {
	vi.useRealTimers();
	document.body.replaceChildren();
});

describe('IrisChrome keys', () => {
	it('dispatch the YouTube / Netflix shortcuts to the engine', async () => {
		const { engine, key, onVolumeChange } = await setup();
		key(' ');
		expect(engine.pause).toHaveBeenCalledOnce();
		key('k');
		expect(engine.play).toHaveBeenCalledOnce();
		key('j');
		expect(engine.seek).toHaveBeenLastCalledWith(590);
		key('ArrowRight');
		expect(engine.seek).toHaveBeenLastCalledWith(600);
		key('l');
		expect(engine.seek).toHaveBeenLastCalledWith(610);
		key('5');
		expect(engine.seek).toHaveBeenLastCalledWith(1650);
		key('ArrowUp');
		expect(engine.setVolume).toHaveBeenLastCalledWith(0.6);
		key('ArrowDown');
		expect(engine.setVolume.mock.lastCall?.[0]).toBeCloseTo(0.5);
		key('m');
		expect(engine.setMuted).toHaveBeenLastCalledWith(true);
		expect(onVolumeChange).toHaveBeenLastCalledWith(0.5, true);
	});

	it('leave a focused control its own keys: a button its Space, the timeline its arrows', async () => {
		const { engine, key, stage } = await setup();
		const seek = stage.querySelector<HTMLElement>('[role="slider"][aria-label="Seek"]')!;
		key('ArrowRight', seek);
		// the slider's own step (5 s), not the player's 10 s
		expect(engine.seek).toHaveBeenCalledExactlyOnceWith(605);
		const button = page.getByRole('button', { name: 'Back 10 seconds, J' }).element();
		key(' ', button);
		expect(engine.pause).not.toHaveBeenCalled();
	});

	it('C opens the audio and subtitles panel, Escape closes it and gives the focus back', async () => {
		const { key, onAudioPick, onSubtitleChange } = await setup();
		const trigger = page.getByRole('button', { name: 'Audio and subtitles, C' });
		await expect.element(trigger).toHaveAttribute('aria-expanded', 'false');
		key('c');
		const dialog = page.getByRole('dialog', { name: 'Audio and subtitles' });
		await expect.element(dialog).toBeVisible();
		await expect.element(trigger).toHaveAttribute('aria-expanded', 'true');
		await expect.element(page.getByRole('group', { name: 'Audio' })).toBeVisible();
		await expect.element(page.getByRole('radio', { name: 'English, original' })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'English, original' })).toHaveFocus();
		await expect.element(page.getByText('Kept for the whole series')).not.toBeInTheDocument();
		await page.getByRole('radio', { name: 'French (VF)' }).click();
		expect(onAudioPick).toHaveBeenCalledWith('1');
		await page.getByRole('radio', { name: 'English, for deaf and hard of hearing (SDH)' }).click();
		expect(onSubtitleChange.mock.lastCall?.[0]?.stream_idx).toBe(3);
		await page.getByRole('radio', { name: 'Off' }).click();
		expect(onSubtitleChange).toHaveBeenLastCalledWith(null);
		await userEvent.keyboard('{Escape}');
		await expect.element(dialog).not.toBeInTheDocument();
		await expect.element(trigger).toHaveFocus();
	});

	it('says where a choice is kept', async () => {
		const { key } = await setup({}, { keptFor: 'Kept for the whole series' });
		key('c');
		await expect.element(page.getByText('Kept for the whole series')).toBeVisible();
	});

	it('lists its shortcuts on ?', async () => {
		const { key } = await setup();
		key('?');
		const list = page.getByRole('dialog', { name: 'Keyboard shortcuts' });
		await expect.element(list).toBeVisible();
		await expect.element(list.getByText('Back 10 seconds')).toBeVisible();
	});
});

describe('IrisChrome sliders', () => {
	it('say the position and the volume in words', async () => {
		// the volume slider leaves narrow screens (a phone has its own volume keys)
		await page.viewport(1024, 768);
		await setup();
		const seek = page.getByRole('slider', { name: 'Seek' });
		await expect.element(seek).toHaveAttribute('aria-valuetext', '10 minutes of 55 minutes');
		await expect.element(seek).toHaveAttribute('aria-valuenow', '600');
		await expect.element(seek).toHaveAttribute('aria-valuemax', '3300');
		const volume = page.getByRole('slider', { name: 'Volume' });
		await expect.element(volume).toHaveAttribute('aria-valuetext', '50 percent');
	});

	it('seek with Home and End', async () => {
		const { engine, key, stage } = await setup();
		const seek = stage.querySelector<HTMLElement>('[aria-label="Seek"]')!;
		key('End', seek);
		expect(engine.seek).toHaveBeenLastCalledWith(3300);
		key('Home', seek);
		expect(engine.seek).toHaveBeenLastCalledWith(0);
	});

	it('label every button with its shortcut', async () => {
		await setup();
		for (const name of [
			'Pause, Space',
			'Back 10 seconds, J',
			'Forward 10 seconds, L',
			'Mute, M',
			'Audio and subtitles, C',
			'Picture in picture, P',
			'Full screen, F',
			'Keyboard shortcuts, ?'
		]) {
			await expect.element(page.getByRole('button', { name })).toBeInTheDocument();
		}
	});

	it('live: no timeline, no skips, the word Live', async () => {
		await setup({ duration: null }, { live: true });
		await expect.element(page.getByText('Live', { exact: true })).toBeVisible();
		expect(page.getByRole('slider', { name: 'Seek' }).query()).toBeNull();
		expect(page.getByRole('button', { name: 'Back 10 seconds, J' }).query()).toBeNull();
	});
});

describe('IrisChrome auto-hide', () => {
	it('hides after a still moment while playing', async () => {
		vi.useFakeTimers();
		const { shown, onVisibleChange, key } = await setup();
		key('x');
		expect(shown()).toBe(true);
		vi.advanceTimersByTime(HIDE_AFTER_MS + 10);
		flushSync();
		expect(shown()).toBe(false);
		expect(onVisibleChange).toHaveBeenLastCalledWith(false);
		key('x');
		expect(shown()).toBe(true);
	});

	it('never hides while paused or while a panel is open', async () => {
		vi.useFakeTimers();
		const paused = await setup({ paused: true });
		vi.advanceTimersByTime(HIDE_AFTER_MS * 3);
		flushSync();
		expect(paused.shown()).toBe(true);
		document.body.replaceChildren();

		const playing = await setup();
		playing.key('c');
		vi.advanceTimersByTime(HIDE_AFTER_MS * 3);
		flushSync();
		expect(playing.shown()).toBe(true);
	});

	it('never hides while a control has the keyboard focus', async () => {
		const { shown, stage } = await setup();
		stage.querySelector<HTMLElement>('[aria-label="Seek"]')!.focus();
		await userEvent.tab();
		vi.useFakeTimers();
		flushSync();
		vi.advanceTimersByTime(HIDE_AFTER_MS * 3);
		flushSync();
		expect(shown()).toBe(true);
	});
});
