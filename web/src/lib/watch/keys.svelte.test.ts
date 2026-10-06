import { afterEach, describe, expect, it } from 'vitest';
import { isTheaterKey } from './keys.ts';

const key = (target: Element, init: KeyboardEventInit) => {
	let event!: KeyboardEvent;
	target.addEventListener('keydown', (e) => (event = e as KeyboardEvent), { once: true });
	target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, ...init }));
	return event;
};

describe('the theater key', () => {
	afterEach(() => (document.body.innerHTML = ''));

	it('is T (either case) without a modifier', () => {
		document.body.innerHTML = '<div id="stage" tabindex="0"></div>';
		const stage = document.getElementById('stage')!;
		expect(isTheaterKey(key(stage, { key: 't' }))).toBe(true);
		expect(isTheaterKey(key(stage, { key: 'T', shiftKey: true }))).toBe(true);
		expect(isTheaterKey(key(stage, { key: 't', ctrlKey: true }))).toBe(false);
		expect(isTheaterKey(key(stage, { key: 'k' }))).toBe(false);
	});

	it('leaves a text field its letter', () => {
		document.body.innerHTML = '<input id="q" />';
		expect(isTheaterKey(key(document.getElementById('q')!, { key: 't' }))).toBe(false);
	});

	it('does nothing behind a modal dialog', () => {
		document.body.innerHTML = '<div id="stage" tabindex="0"></div><div role="dialog" aria-modal="true"></div>';
		expect(isTheaterKey(key(document.getElementById('stage')!, { key: 't' }))).toBe(false);
	});
});
