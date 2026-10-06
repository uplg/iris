<script lang="ts">
	// Live TV: a country's channels (TNT numbers first, then by category), each with what is on
	// now and next, and a search across every country. The country is a tab, kept in the URL
	// without a navigation (the focus stays on the tab).
	import { createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { livetv, type LiveChannel, type LiveNowNext } from '@iris/api/client';
	import PageHead from '#lib/components/PageHead.svelte';
	import Tabs from '#lib/components/Tabs.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { loadable } from '#lib/query.ts';
	import ChannelTile from './ChannelTile.svelte';

	/** The guide changes on programme boundaries: a minute keeps the bars honest. */
	const EPG_REFETCH_MS = 60_000;

	const countriesQ = createQuery(() => ({
		queryKey: ['livetv', 'countries'],
		queryFn: () => livetv.countries(),
		staleTime: 24 * 3600_000
	}));
	let picked = $state<string | null>(untrack(() => page.url.searchParams.get('country')));
	const country = $derived(picked ?? countriesQ.data?.default_country ?? 'fr');

	function pick(c: string) {
		picked = c;
		const url = new URL(page.url.href);
		url.searchParams.set('country', c);
		replaceState(url, {});
	}

	const channelsQ = createQuery(() => ({
		queryKey: ['livetv', 'channels', country],
		queryFn: () => livetv.channels(country),
		staleTime: 10 * 60_000
	}));
	const epgQ = createQuery(() => ({
		queryKey: ['livetv', 'epg-now', country],
		queryFn: () => livetv.epgNow(country),
		refetchInterval: EPG_REFETCH_MS
	}));
	const epg = $derived(new Map<string, LiveNowNext>((epgQ.data?.entries ?? []).map((e) => [e.channel_id, e])));
	// « now » for the bars: when the guide was last read (it moves with each refetch)
	const at = $derived(epgQ.dataUpdatedAt || Date.now());

	// across countries, server-side (an in-memory index: one query per keystroke is cheap)
	let query = $state('');
	const q = $derived(query.trim());
	const searchQ = createQuery(() => ({
		queryKey: ['livetv', 'search', q],
		queryFn: () => livetv.search(q),
		enabled: q.length >= 2,
		staleTime: 60_000,
		placeholderData: keepPreviousData
	}));

	const sections = $derived.by(() => {
		const channels = channelsQ.data?.channels ?? [];
		const byCategory = new Map<string, LiveChannel[]>();
		for (const c of channels) {
			if (typeof c.tnt_number === 'number') continue;
			const key = c.categories[0] ?? 'Other';
			const bucket = byCategory.get(key) ?? [];
			bucket.push(c);
			byCategory.set(key, bucket);
		}
		return { tnt: channels.filter((c) => typeof c.tnt_number === 'number'), categories: [...byCategory.entries()] };
	});
	const countryName = (code: string) => countriesQ.data?.countries.find((c) => c.code === code)?.name ?? code.toUpperCase();
</script>

{#snippet grid(channels: LiveChannel[], ctry: string, showNumber: boolean, cross: boolean)}
	<ul class="grid">
		{#each channels as c (c.id)}
			<li>
				<ChannelTile
					channel={c}
					country={ctry}
					nowNext={cross ? undefined : epg.get(c.id)}
					{at}
					{showNumber}
					countryLabel={cross ? countryName(ctry) : undefined}
				/>
			</li>
		{/each}
	</ul>
{/snippet}

<div class="live">
	<PageHead title="Live TV">
		{#snippet end()}
			<label class="search">
				<span class="sr-only">Search channels in every country</span>
				<input type="search" bind:value={query} placeholder="Search channels, every country" autocomplete="off" />
			</label>
		{/snippet}
	</PageHead>

	{#if q.length >= 2}
		<section aria-labelledby="results-title" class="group">
			<h2 id="results-title" class="group-title">Channels matching “{q}”</h2>
			{#if searchQ.isPending}
				<p class="hint">Searching…</p>
			{:else if (searchQ.data?.results ?? []).length === 0}
				<p class="hint" role="status">No channel matches “{q}”.</p>
			{:else}
				<p class="sr-only" role="status">{searchQ.data?.results.length} channels found.</p>
				{#each [...new Set(searchQ.data!.results.map((r) => r.country))] as ctry (ctry)}
					{@render grid(
						searchQ
							.data!.results.filter((r) => r.country === ctry)
							.map((r) => ({
								id: r.id,
								name: r.name,
								logo_url: r.logo_url ?? null,
								logo_origin: r.logo_origin ?? null,
								categories: [],
								geo_blocked: false,
								not_24_7: false,
								quality: null,
								tnt_number: null
							})),
						ctry,
						false,
						true
					)}
				{/each}
			{/if}
		</section>
	{:else}
		<Loaded value={loadable(countriesQ)}>
			<Tabs
				tabs={(countriesQ.data?.countries ?? []).map((c) => ({ value: c.code, label: `${c.flag} ${c.name}` }))}
				value={country}
				onchange={pick}
			>
				{#snippet panel(value)}
					{#if value === country}
						<Loaded
							value={loadable(channelsQ)}
							empty={!sections.tnt.length && !sections.categories.length}
							emptyText="No channels for this country."
						>
							{#if sections.tnt.length}
								<section class="group" aria-labelledby="tnt-title">
									<h2 id="tnt-title" class="group-title">Free-to-air (TNT)</h2>
									{@render grid(sections.tnt, country, true, false)}
								</section>
							{/if}
							{#each sections.categories as [title, channels], i (title)}
								<section class="group" aria-labelledby="cat-{i}">
									<h2 id="cat-{i}" class="group-title">{title}</h2>
									{@render grid(channels, country, false, false)}
								</section>
							{/each}
						</Loaded>
					{/if}
				{/snippet}
			</Tabs>
		</Loaded>
	{/if}
</div>

<style>
	.live :global(.tabs-list) {
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.live :global(.tabs-trigger) {
		white-space: nowrap;
	}
	.search input {
		width: min(20rem, calc(100vw - 2 * var(--page-margin)));
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(11rem, 100%), 1fr));
		gap: var(--s-3);
	}
</style>
