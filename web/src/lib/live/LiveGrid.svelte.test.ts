import axe from 'axe-core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import '../../styles/app.css';
import { STORAGE } from '#lib/storage.ts';
import { stubApi } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import Provided from '#lib/home/test/Provided.svelte';
import LiveGrid from './LiveGrid.svelte';
import { liveRoutes, STATES } from './fixtures.ts';

const route = vi.hoisted(() => ({ url: new URL('http://iris.test/live') }));
const nav = vi.hoisted(() => ({ replaceState: vi.fn(), goto: vi.fn() }));
vi.mock('$app/state', () => ({ page: route }));
vi.mock('$app/navigation', () => nav);

const at = (path: string) => (route.url = new URL(path, 'http://iris.test'));
const show = () => render(Provided, { props: { view: LiveGrid } });
const picker = () => page.getByRole('combobox', { name: 'Country' });
const listing = () => page.getByRole('region', { name: /^Channels in/ });
const lastUrl = () => String(nav.replaceState.mock.calls.at(-1)?.[0] ?? '');
const kept = () => JSON.parse(localStorage.getItem(STORAGE.liveCountries) ?? '[]') as string[];

describe('Live TV', () => {
	beforeEach(() => {
		localStorage.clear();
		nav.replaceState.mockClear();
		at('/live');
	});

	it('opens on the household’s country: free-to-air by number, what is on now and next', async () => {
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(page.getByRole('heading', { name: 'Channels in France · 40 channels' })).toBeVisible();
		await expect.element(picker()).toHaveValue('France');
		const tf1 = page.getByRole('link', { name: /^Channel 1 TF1/ });
		await expect.element(tf1).toHaveAttribute('href', '/live/fr/tf1');
		const words = () => tf1.element().textContent?.replace(/\s+/g, ' ') ?? '';
		await expect.poll(words).toContain('Now: Le journal de 20 h');
		expect(words()).toMatch(/Next at \d\d:\d\d: Le Grand/);
		await expect.element(page.getByRole('link', { name: /Trace Urban Not answering right now/ })).toBeVisible();
		await expect.element(page.getByRole('link', { name: /^Euronews .*May be blocked in your country$/ })).toBeVisible();
	});

	it('picks a country by typing its name: kept in the URL, remembered, said', async () => {
		const api = stubApi(liveRoutes(Date.now()));
		await show();
		await picker().click();
		await userEvent.keyboard('united s');
		await expect.element(page.getByRole('option', { name: /United States/ })).toBeVisible();
		await userEvent.keyboard('{Enter}');
		await expect.element(page.getByRole('heading', { name: 'Channels in United States · 300 channels' })).toBeVisible();
		expect(lastUrl()).toContain('country=us');
		expect(kept()).toEqual(['us']);
		expect(ui.polite).toBe('United States, 300 channels.');
		expect(api.sent('GET', '/livetv/us/channels')).toHaveLength(1);
		await expect.element(picker()).toHaveValue('United States');
		expect(document.activeElement).toBe(picker().element());
	});

	it('is operable by the keyboard alone: arrows through the matches, Enter picks', async () => {
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(picker()).toHaveValue('France');
		(picker().element() as HTMLInputElement).focus();
		await userEvent.keyboard('uni');
		await expect.element(page.getByRole('option', { name: /United Arab Emirates/ })).toBeVisible();
		await userEvent.keyboard('{ArrowDown}{Enter}');
		await expect.element(picker()).toHaveValue('United Kingdom');
		expect(lastUrl()).toContain('country=gb');
	});

	it('shows the usual countries first: the household’s, then the last picked', async () => {
		localStorage.setItem(STORAGE.liveCountries, JSON.stringify(['us', 'de']));
		stubApi(liveRoutes(Date.now()));
		await show();
		// the last pick comes back in this browser
		await expect.element(picker()).toHaveValue('United States');
		await page.getByRole('button', { name: 'Show every country' }).click();
		const usual = page.getByRole('group', { name: 'Your countries' });
		await expect.element(usual).toBeVisible();
		expect(
			usual
				.getByRole('option')
				.elements()
				.map((o) => o.textContent?.replace(/\s+/g, ' ').trim())
		).toEqual(['🇫🇷 France, 40 channels', '🇺🇸 United States, 300 channels', '🇩🇪 Germany, 172 channels']);
		const all = page.getByRole('group', { name: 'All countries' }).getByRole('option').elements();
		expect(all[0].textContent).toContain('Afghanistan');
		expect(all.some((o) => o.textContent?.includes('France'))).toBe(false);
	});

	it('a country given in the URL wins over the remembered one', async () => {
		localStorage.setItem(STORAGE.liveCountries, JSON.stringify(['us']));
		at('/live?country=fr');
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(picker()).toHaveValue('France');
	});

	it('filters one category at a time, the count of each said', async () => {
		stubApi(liveRoutes(Date.now()));
		await show();
		const filter = page.getByRole('group', { name: 'Show' });
		await expect.element(filter.getByRole('radio', { name: /All\s+40/ })).toBeChecked();
		(filter.getByRole('radio', { name: /Music/ }).element() as HTMLInputElement).click();
		await expect.element(listing().getByRole('heading', { level: 3, name: 'Music · 3' })).toBeVisible();
		expect(listing().getByRole('heading', { level: 3 }).elements()).toHaveLength(1);
		expect(listing().getByRole('link').elements()).toHaveLength(3);
		(filter.getByRole('radio', { name: /All/ }).element() as HTMLInputElement).click();
		await expect.element(listing().getByRole('heading', { name: 'Free-to-air (TNT) · 25' })).toBeVisible();
	});

	it('a country without a guide lists its channels and says so', async () => {
		at('/live?country=us');
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(page.getByText('No programme guide for this country: channels show without what is on.')).toBeVisible();
		expect(listing().getByRole('link').elements()).toHaveLength(STATES.length);
	});

	it('a country with no channels yet says so, in words', async () => {
		at('/live?country=ad');
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(page.getByText('No channels for this country yet.')).toBeVisible();
		await expect.element(page.getByRole('heading', { name: 'Channels in Andorra' })).toBeVisible();
	});

	it('searches every country, the results grouped by their country', async () => {
		const api = stubApi(
			liveRoutes(Date.now(), {
				'/livetv/search?q=france': {
					results: [
						{ country: 'fr', id: 'france2', name: 'France 2' },
						{ country: 'be', id: 'france24', name: 'France 24' }
					]
				}
			})
		);
		await show();
		await page.getByRole('searchbox', { name: 'Search every country' }).fill('france');
		await expect.element(page.getByRole('heading', { name: 'Channels matching “france” in every country' })).toBeVisible();
		await expect.element(page.getByRole('status').getByText('2 channels in 2 countries.')).toBeVisible();
		const belgium = page.getByRole('region', { name: '🇧🇪 Belgium' });
		await expect.element(belgium.getByRole('link', { name: /France 24/ })).toHaveAttribute('href', '/live/be/france24');
		expect(api.sent('GET', '/livetv/search?q=france')).toHaveLength(1);
		expect(listing().elements()).toHaveLength(0);
	});

	it('says when nothing matches anywhere', async () => {
		stubApi(liveRoutes(Date.now(), { '/livetv/search?q=zzz': { results: [] } }));
		await show();
		await page.getByRole('searchbox', { name: 'Search every country' }).fill('zzz');
		await expect.element(page.getByText('No channel matches “zzz”, in any country.')).toBeVisible();
	});

	it('a failed channel list says so and offers a retry', async () => {
		stubApi(liveRoutes(Date.now(), { '/livetv/fr/channels': new Response('{}', { status: 502 }) }));
		await show();
		await expect.element(page.getByText('This could not be loaded.')).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Try again' })).toBeVisible();
	});

	it('passes axe, the country list open too', async () => {
		stubApi(liveRoutes(Date.now()));
		await show();
		await expect.element(page.getByRole('link', { name: /^Channel 1 TF1/ })).toBeVisible();
		expect((await axe.run(document.body)).violations.map((v) => v.id)).toEqual([]);
		await page.getByRole('button', { name: 'Show every country' }).click();
		await expect.element(page.getByRole('group', { name: 'All countries' })).toBeVisible();
		// the open list scrolls while the focus stays in the field (aria-activedescendant): what
		// axe asks of a scrolling region does not apply to a combobox popup
		const open = await axe.run(document.body, { rules: { 'scrollable-region-focusable': { enabled: false } } });
		expect(open.violations.map((v) => v.id)).toEqual([]);
	});

	it('fits a 320 px screen, the category filter one line that scrolls', async () => {
		stubApi(liveRoutes(Date.now()));
		await page.viewport(320, 2400);
		await show();
		await expect.element(page.getByRole('link', { name: /^Channel 1 TF1/ })).toBeVisible();
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		const pills = page.getByRole('group', { name: 'Show' }).getByRole('radio').elements();
		const tops = new Set(pills.map((p) => Math.round(p.closest('label')!.getBoundingClientRect().top)));
		expect(tops.size).toBe(1);
	});
});
