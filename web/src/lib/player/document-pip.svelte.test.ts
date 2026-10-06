import { describe, expect, it, vi } from 'vitest';
import { DocumentPip } from './document-pip.svelte.ts';

describe('DocumentPip', () => {
	it('closes the open window and forgets it', () => {
		const pip = new DocumentPip();
		const close = vi.fn();
		pip.window = { close, innerWidth: 640, innerHeight: 360 } as unknown as Window;
		pip.close();
		expect(close).toHaveBeenCalledTimes(1);
		expect(pip.window).toBeNull();
		expect(pip.active).toBe(false);
	});

	it('has nothing to do without a window', () => {
		const pip = new DocumentPip();
		expect(() => pip.close()).not.toThrow();
		expect(pip.window).toBeNull();
	});
});
