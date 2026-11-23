import { describe, expect, it, vi } from 'vitest';
import { isFullscreen, keepScreenAwake, PAGE_FULLSCREEN_CHANGE, toggleFullscreen } from './browser.ts';

function stage(over: Record<string, unknown> = {}, doc: Record<string, unknown> = {}) {
	const el = {
		dataset: {} as DOMStringMap,
		dispatchEvent: vi.fn(),
		querySelector: vi.fn(() => null),
		ownerDocument: { fullscreenElement: null, exitFullscreen: vi.fn(), documentElement: { dataset: {} as DOMStringMap }, ...doc },
		...over
	};
	return el as unknown as HTMLElement & { dispatchEvent: ReturnType<typeof vi.fn> };
}
const root = (el: HTMLElement) => el.ownerDocument.documentElement.dataset;

describe('toggleFullscreen', () => {
	it('puts the stage in full screen where elements can go there', async () => {
		const requestFullscreen = vi.fn();
		const el = stage({ requestFullscreen });
		await toggleFullscreen(el, { overlay: true });
		expect(requestFullscreen).toHaveBeenCalledTimes(1);
		expect(el.querySelector).not.toHaveBeenCalled();
		expect(el.dataset.pageFullscreen).toBeUndefined();
	});

	it('leaves it when the stage is the full screen element', async () => {
		const exitFullscreen = vi.fn();
		const el = stage({ requestFullscreen: vi.fn() });
		Object.assign(el.ownerDocument, { fullscreenElement: el, exitFullscreen });
		expect(isFullscreen(el)).toBe(true);
		await toggleFullscreen(el);
		expect(exitFullscreen).toHaveBeenCalledTimes(1);
	});

	it('hands the video to the system player on an iPhone', async () => {
		const webkitEnterFullscreen = vi.fn();
		const el = stage({ querySelector: vi.fn(() => ({ webkitEnterFullscreen })) });
		await toggleFullscreen(el);
		expect(el.querySelector).toHaveBeenCalledWith('video');
		expect(webkitEnterFullscreen).toHaveBeenCalledTimes(1);
		expect(isFullscreen(el)).toBe(false);
	});

	it('keeps the stage in the page when subtitles are drawn on it, and leaves on the next call', async () => {
		const webkitEnterFullscreen = vi.fn();
		const el = stage({ querySelector: vi.fn(() => ({ webkitEnterFullscreen })) });
		await toggleFullscreen(el, { overlay: true });
		expect(webkitEnterFullscreen).not.toHaveBeenCalled();
		expect(isFullscreen(el)).toBe(true);
		expect(root(el).stageFullscreen).toBe('');
		expect(el.dispatchEvent.mock.calls[0][0].type).toBe(PAGE_FULLSCREEN_CHANGE);

		await toggleFullscreen(el);
		expect(isFullscreen(el)).toBe(false);
		expect(root(el).stageFullscreen).toBeUndefined();
		expect(el.dispatchEvent).toHaveBeenCalledTimes(2);
	});

	it('takes the page too when there is no video to hand over', async () => {
		const el = stage();
		await toggleFullscreen(el);
		expect(isFullscreen(el)).toBe(true);
	});

	it('does nothing when the system refuses', async () => {
		const refused = () => ({
			webkitEnterFullscreen: () => {
				throw new DOMException('no metadata yet', 'InvalidStateError');
			}
		});
		const el = stage({ querySelector: refused });
		await expect(toggleFullscreen(el)).resolves.toBeUndefined();
		expect(isFullscreen(el)).toBe(false);
	});
});

describe('keepScreenAwake', () => {
	function page(visibilityState = 'visible') {
		const listeners = new Set<() => void>();
		const release = vi.fn(async () => undefined);
		const request = vi.fn(async () => ({ release }));
		const doc = {
			visibilityState,
			defaultView: { navigator: { wakeLock: { request } } },
			addEventListener: (_: string, f: () => void) => listeners.add(f),
			removeEventListener: (_: string, f: () => void) => listeners.delete(f)
		};
		const show = (state: string) => {
			doc.visibilityState = state;
			for (const f of listeners) f();
		};
		return { doc: doc as unknown as Document, request, release, show, listeners };
	}
	const settled = () => new Promise<void>((r) => r());

	it('takes the screen lock and gives it back', async () => {
		const p = page();
		const release = keepScreenAwake(p.doc);
		expect(p.request).toHaveBeenCalledWith('screen');
		await settled();
		release();
		expect(p.release).toHaveBeenCalledTimes(1);
		expect(p.listeners.size).toBe(0);
	});

	it('takes it again when the page comes back', async () => {
		const p = page();
		const release = keepScreenAwake(p.doc);
		p.show('hidden');
		expect(p.request).toHaveBeenCalledTimes(1);
		p.show('visible');
		expect(p.request).toHaveBeenCalledTimes(2);
		release();
	});

	it('gives back a lock granted after the release', async () => {
		const p = page();
		keepScreenAwake(p.doc)();
		await settled();
		await settled();
		expect(p.release).toHaveBeenCalledTimes(1);
	});

	it('is a no-op where the browser has no wake lock', () => {
		const doc = { defaultView: { navigator: {} } } as unknown as Document;
		expect(() => keepScreenAwake(doc)()).not.toThrow();
	});
});
