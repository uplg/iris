import { afterEach, describe, expect, it } from 'vitest';
import { page as screen } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { CLIENT_OUTDATED_EVENT } from '@iris/api/client';
import Outdated from '#lib/components/Outdated.svelte';
import { harmlessReload, outdated } from './update.svelte.ts';

describe('a deploy reloads only at a harmless moment', () => {
	afterEach(() => (document.body.innerHTML = ''));

	it('on a page where nothing plays and nobody types', () => {
		expect(harmlessReload('/library')).toBe(true);
		expect(harmlessReload('/live')).toBe(true);
	});

	it('never on a player’s page (its picture-in-picture included)', () => {
		expect(harmlessReload('/watch/abc/0')).toBe(false);
		expect(harmlessReload('/live/fr/tf1')).toBe(false);
	});

	it('never while typing', () => {
		document.body.innerHTML = '<input id="q" />';
		document.getElementById('q')!.focus();
		expect(harmlessReload('/search')).toBe(false);
	});

	it('never while a media element plays', () => {
		document.body.innerHTML = '<video></video>';
		const video = document.querySelector('video')!;
		Object.defineProperty(video, 'paused', { value: false });
		expect(harmlessReload('/')).toBe(false);
	});
});

describe('a server that refuses this bundle', () => {
	it('locks the app on its 426, offering the reload and the account page', async () => {
		const stop = outdated.listen();
		expect(outdated.locked).toBe(false);
		window.dispatchEvent(new Event(CLIENT_OUTDATED_EVENT));
		expect(outdated.locked).toBe(true);
		stop();
		outdated.locked = false;

		await render(Outdated);
		await expect.element(screen.getByRole('heading', { level: 1, name: 'This server needs a newer Iris' })).toBeVisible();
		await expect.element(screen.getByRole('button', { name: 'Reload' })).toBeVisible();
		await expect.element(screen.getByRole('link', { name: 'Account' })).toHaveAttribute('href', '/account');
	});
});
