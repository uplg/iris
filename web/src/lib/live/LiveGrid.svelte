<script lang="ts">
	// Live TV as a guide: one country at a time (picked by name, kept in the URL, the last few
	// remembered in this browser), its channels in sections (free-to-air by number, then by
	// category, one of them alone through the filter), each with what is on now and next; and a
	// search that looks in every country.
	import { createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { livetv, type LiveChannel, type LiveCountry, type LiveNowNext, type LiveSearchResult } from '@iris/api/client';
	import { plural } from '@iris/api/format';
	import PageHead from '#lib/components/PageHead.svelte';
	import PillChoice from '#lib/components/PillChoice.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { read } from '#lib/queries.ts';
	import { loadable } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import ChannelRow from './ChannelRow.svelte';
	import CountryPicker from './CountryPicker.svelte';
	import { channelCount, channelSections, recentCountries, remember, usualCountries } from './guide.ts';

	/** The guide changes on programme boundaries: a minute keeps the bars honest. */
	const EPG_REFETCH_MS = 60_000;
	const ALL = 'all';

	const countriesQ = createQuery(() => read.liveCountries());
	const offered = $derived(countriesQ.data?.countries ?? []);
	let recent = $state(untrack(() => recentCountries.get()));
	let picked = $state<string | null>(untrack(() => page.url.searchParams.get('country')));
	const country = $derived.by(() => {
		if (picked) return picked;
		const kept = recent.find((c) => offered.some((o) => o.code === c));
		return kept ?? countriesQ.data?.default_country ?? 'fr';
	});
	const usual = $derived(usualCountries(countriesQ.data?.default_country ?? 'fr', recent, offered));
	const countryOf = (code: string): LiveCountry | undefined => offered.find((c) => c.code === code);
	const countryName = (code: string) => countryOf(code)?.name ?? code.toUpperCase();

	let category = $state(ALL);

	function pick(code: string) {
		picked = code;
		category = ALL;
		recent = remember(recent, code);
		recentCountries.set(recent);
		const url = new URL(page.url.href);
		url.searchParams.set('country', code);
		replaceState(url, {});
		const c = countryOf(code);
		const count = c ? channelCount(c) : null;
		ui.say(count ? `${countryName(code)}, ${count}.` : `${countryName(code)}.`);
	}

	const channelsQ = createQuery(() => read.liveChannels(country));
	const epgQ = createQuery(() => ({ ...read.liveEpg(country), refetchInterval: EPG_REFETCH_MS }));
	const epg = $derived(new Map<string, LiveNowNext>((epgQ.data?.entries ?? []).map((e) => [e.channel_id, e])));
	const guided = $derived(epg.size > 0);
	// « now » for the bars: when the guide was last read (it moves with each refetch)
	const at = $derived(epgQ.dataUpdatedAt || Date.now());

	const channels = $derived(channelsQ.data?.channels ?? []);
	const sections = $derived(channelSections(channels));
	const filtered = $derived(sections.some((s) => s.key === category) ? category : ALL);
	const shown = $derived(filtered === ALL ? sections : sections.filter((s) => s.key === filtered));
	const filters = $derived([
		{ value: ALL, label: 'All', count: channels.length },
		...sections.map((s) => ({ value: s.key, label: s.title, count: s.channels.length }))
	]);

	// across countries, server-side (an in-memory index: one query per keystroke is cheap)
	let query = $state('');
	const q = $derived(query.trim());
	const searching = $derived(q.length >= 2);
	const searchQ = createQuery(() => ({
		queryKey: ['livetv', 'search', q],
		queryFn: () => livetv.search(q),
		enabled: q.length >= 2,
		staleTime: 60_000,
		placeholderData: keepPreviousData
	}));
	const hits = $derived(searchQ.data?.results ?? []);
	const hitsByCountry = $derived.by(() => {
		const groups = new Map<string, LiveSearchResult[]>();
		for (const r of hits) groups.set(r.country, [...(groups.get(r.country) ?? []), r]);
		return [...groups.entries()];
	});
	const asChannel = (r: LiveSearchResult): LiveChannel => ({
		id: r.id,
		name: r.name,
		logo_url: r.logo_url ?? null,
		logo_origin: r.logo_origin ?? null,
		categories: [],
		geo_blocked: false,
		not_24_7: false,
		quality: null,
		tnt_number: null
	});
	const flagged = (code: string) => {
		const c = countryOf(code);
		return c ? `${c.flag} ${c.name}` : code.toUpperCase();
	};
</script>

<div class="live">
	<PageHead title="Live TV" />

	<div class="toolbar">
		<Loaded value={loadable(countriesQ)} empty={!offered.length} emptyText="No country has channels right now.">
			<CountryPicker countries={offered} {usual} value={country} onchange={pick} />
		</Loaded>
		<div class="field search">
			<label class="label" for="live-search">Search every country</label>
			<input id="live-search" type="search" bind:value={query} placeholder="A channel’s name" autocomplete="off" />
		</div>
	</div>

	{#if searching}
		<section aria-labelledby="results-title" class="results">
			<h2 id="results-title" class="group-title">Channels matching “{q}” in every country</h2>
			{#if searchQ.isPending}
				<p class="hint">Searching…</p>
			{:else if searchQ.isError}
				<p class="hint" role="status">The search could not be done. Try again in a moment.</p>
			{:else if !hits.length}
				<p class="hint" role="status">No channel matches “{q}”, in any country.</p>
			{:else}
				<p class="hint" role="status">{plural(hits.length, 'channel')} in {plural(hitsByCountry.length, 'country', 'countries')}.</p>
				{#each hitsByCountry as [code, results] (code)}
					<section class="section" aria-labelledby="hits-{code}">
						<h3 class="section-title" id="hits-{code}">{flagged(code)}</h3>
						<ul class="rows compact">
							{#each results as r (r.id)}
								<li><ChannelRow channel={asChannel(r)} country={code} {at} /></li>
							{/each}
						</ul>
					</section>
				{/each}
			{/if}
		</section>
	{:else if countriesQ.isSuccess && offered.length}
		<section aria-labelledby="channels-title" class="listing">
			<h2 id="channels-title" class="group-title">
				Channels in {countryName(country)}{#if channels.length}<span class="muted count">{` · ${plural(channels.length, 'channel')}`}</span
					>{/if}
			</h2>
			<Loaded
				value={loadable(channelsQ)}
				empty={!channels.length}
				emptyText="No channels for this country yet."
				emptyHint="Pick another country, or search every country by a channel’s name."
			>
				{#if sections.length > 1}
					<div class="filters">
						<PillChoice legend="Show" options={filters} value={filtered} onchange={(v) => (category = v)} />
					</div>
				{/if}
				{#if !guided && epgQ.isFetched}
					<p class="hint">No programme guide for this country: channels show without what is on.</p>
				{/if}
				{#each shown as s, i (s.key)}
					<section class="section" aria-labelledby="sec-{i}">
						<h3 class="section-title" id="sec-{i}">
							{s.title}<span class="muted count">{` · ${s.channels.length}`}</span>
						</h3>
						<ul class="rows" class:compact={!guided}>
							{#each s.channels as c (c.id)}
								<li><ChannelRow channel={c} {country} nowNext={epg.get(c.id)} {at} {guided} /></li>
							{/each}
						</ul>
					</section>
				{/each}
			</Loaded>
		</section>
	{/if}
</div>

<style>
	.live {
		display: grid;
		gap: var(--s-5);
	}
	.toolbar {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 18rem), 1fr));
		gap: var(--s-3) var(--s-4);
		max-width: 48rem;
	}
	.search input {
		width: 100%;
	}
	.listing,
	.results {
		display: grid;
		gap: var(--s-4);
		min-width: 0;
	}
	.listing > :global(h2),
	.results > h2 {
		margin: 0;
	}
	.count {
		font: var(--t-secondary);
	}
	.filters {
		min-width: 0;
	}
	.section {
		display: grid;
		gap: var(--s-3);
	}
	.section + .section {
		margin-top: var(--s-3);
	}
	.section-title {
		margin: 0;
		font: var(--t-label);
		font-size: 1rem;
	}
	.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(100%, 22rem), 1fr));
		gap: var(--s-2) var(--s-3);
	}
	/* without a guide a channel is a name: more of them to a line */
	.rows.compact {
		grid-template-columns: repeat(auto-fill, minmax(min(100%, 16rem), 1fr));
	}
	/* on a phone the filter is one line that scrolls sideways, not a wall above the channels */
	@media (max-width: 40rem) {
		/* positioned: the pills' visually hidden radios stay inside the scrolling line */
		.filters :global(.row) {
			position: relative;
			flex-wrap: nowrap;
			overflow-x: auto;
			scrollbar-width: none;
			margin-inline: calc(-1 * var(--page-margin));
			padding: var(--s-1) var(--page-margin);
		}
		.filters :global(.pill-btn) {
			flex: none;
		}
	}
	.hint {
		margin: 0;
	}
</style>
