import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { session } from '#lib/session.svelte.ts';
import { ui } from '#lib/ui.svelte.ts';
import { stubApi } from '#lib/test/api.ts';
import '../../styles/app.css';
import { alex, leonard, noContent } from '../account/testing.ts';
import { seleo } from '../admin/testing.ts';
import SeleoShare from './SeleoShare.svelte';

const film = seleo[0];
const shares = `/admin/seleo/shares/${film.infohash}/0`;

describe('sharing a film with SéléoTV', () => {
	afterEach(() => {
		ui.toasts = [];
	});

	it('shares it for a week, then says until when, as the server answered', async () => {
		session.signedIn(leonard);
		const api = stubApi({ 'GET /admin/seleo/shares': [], [`PUT ${shares}`]: film });
		await render(SeleoShare, { infohash: film.infohash, fileIdx: 0, title: '1917' });
		const share = page.getByRole('button', { name: 'Share with SéléoTV' });
		await expect.element(share).toHaveAccessibleDescription('For a week; it stays on disk meanwhile.');
		api.routes['GET /admin/seleo/shares'] = [film];
		await share.click();
		await expect.poll(() => api.sent('PUT', shares)[0]?.body).toEqual({ days: 7 });
		await expect.element(page.getByText(/^On SéléoTV until /)).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Extend 1917 on SéléoTV by a week' })).toBeVisible();
	});

	it('stops sharing it', async () => {
		session.signedIn(leonard);
		const api = stubApi({ 'GET /admin/seleo/shares': [film], [`DELETE ${shares}`]: noContent });
		await render(SeleoShare, { infohash: film.infohash, fileIdx: 0, title: '1917' });
		api.routes['GET /admin/seleo/shares'] = [];
		await page.getByRole('button', { name: 'Stop sharing 1917 with SéléoTV' }).click();
		await expect.poll(() => api.sent('DELETE', shares)).toHaveLength(1);
		await expect.element(page.getByRole('button', { name: 'Share with SéléoTV' })).toBeVisible();
	});

	it('is not there for a member, who asks nothing', async () => {
		session.signedIn(alex);
		const api = stubApi({});
		await render(SeleoShare, { infohash: film.infohash, fileIdx: 0, title: '1917' });
		expect(document.querySelector('.seleo')).toBeNull();
		expect(api.calls).toEqual([]);
	});
});
