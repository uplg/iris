<script lang="ts">
	// The episodes of a series: one tab per season (Specials last in the order of the numbers,
	// never the one it opens on), the season's pack offers above its list; a fleuve anime
	// (`numbering: absolute`) is one flat list on the absolute number, no seasons.
	import type { CollectionDetail, TorrentView } from '@iris/api/client';
	import Tabs from '#lib/components/Tabs.svelte';
	import EpisodeList from './EpisodeList.svelte';
	import SeasonPack from './SeasonPack.svelte';
	import {
		count,
		firstSeason,
		mergeEpisodes,
		mergeEpisodesAbsolute,
		ownedEp,
		seasonName,
		seasonsOf,
		watchedEp,
		type Episode
	} from './merge.ts';

	let { collection: c, torrents }: { collection: CollectionDetail; torrents: Map<string, TorrentView> } = $props();
	const id = $props.id();

	const absolute = $derived(c.numbering === 'absolute');
	const flat = $derived(absolute ? mergeEpisodesAbsolute(c.episodes, c.available_episodes, c.gone_episodes) : []);
	const seasons = $derived(absolute ? [] : seasonsOf(mergeEpisodes(c.episodes, c.available_episodes, c.gone_episodes), c.season_packs));

	let chosen = $state<string | null>(null);
	const active = $derived(chosen && seasons.some((s) => `s${s.season}` === chosen) ? chosen : `s${firstSeason(seasons)}`);
	const current = $derived(seasons.find((s) => `s${s.season}` === active));

	const tabs = $derived(
		seasons.map((s) => {
			const all = s.items.length > 0 && s.items.every(watchedEp);
			return { value: `s${s.season}`, label: `${seasonName(s.season)} · ${all ? 'watched' : count(s.items.length, 'episode')}` };
		})
	);
	const fact = (items: Episode[]) => `${count(items.length, 'episode')} · ${items.filter(ownedEp).length} on disk`;
</script>

<section class="episodes" aria-labelledby="{id}-title">
	<div class="head">
		<h2 id="{id}-title" class="group-title" tabindex="-1">Episodes</h2>
		{#if absolute && flat.length}<span class="hint">{fact(flat)}</span>{/if}
		{#if !absolute && current && current.items.length}<span class="hint">{fact(current.items)}</span>{/if}
	</div>

	{#if absolute}
		{#if flat.length}
			<EpisodeList collectionId={c.id} episodes={flat} {torrents} />
		{:else}
			<p class="hint">No episode found yet for this series.</p>
		{/if}
	{:else if !seasons.length}
		<p class="hint">No episode found yet for this series.</p>
	{:else if seasons.length === 1}
		{@render season(seasons[0].season)}
	{:else}
		<div class="strip">
			<Tabs {tabs} value={active} onchange={(v) => (chosen = v)}>
				{#snippet panel(v)}
					{#if v === active}{@render season(Number(v.slice(1)))}{/if}
				{/snippet}
			</Tabs>
		</div>
	{/if}
</section>

{#snippet season(n: number)}
	{@const s = seasons.find((x) => x.season === n)}
	{#if s}
		{#each s.packs as p (`${p.season}-${p.language ?? '_'}-${p.indexer_torrent_id}`)}
			<SeasonPack collectionId={c.id} pack={p} />
		{/each}
		{#if s.items.length}
			{#key n}<EpisodeList collectionId={c.id} episodes={s.items} {torrents} />{/key}
		{:else}
			<p class="hint">No single episode is available on its own yet. The season pack above brings every episode in one go.</p>
		{/if}
	{/if}
{/snippet}

<style>
	.episodes {
		display: grid;
		gap: var(--s-4);
		max-width: var(--measure-wide);
		min-width: 0;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: var(--s-2) var(--s-4);
	}
	.head h2 {
		font: var(--t-page);
	}
	/* twenty seasons scroll in their row, never widen the page */
	.strip :global(.tabs-list) {
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.strip :global(.tabs-trigger) {
		white-space: nowrap;
	}
</style>
