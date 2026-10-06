<script lang="ts">
	// The library: everything on this server, as titles (a poster grid) or as releases
	// (downloading, needing a hand, seeding). The view is remembered per browser.
	import PageHead from '#lib/components/PageHead.svelte';
	import { stored, text } from '#lib/stored.ts';
	import { createQuery } from '@tanstack/svelte-query';
	import { collectionsOf, read } from '#lib/queries.ts';
	import { LIBRARY_VIEW_KEY, titleCounts, VIEWS, type View } from './model.ts';
	import TitlesView from './TitlesView.svelte';
	import DownloadsView from './DownloadsView.svelte';

	const kept = stored<View>(LIBRARY_VIEW_KEY, 'titles', text(VIEWS));
	let view = $state<View>(kept.get());
	const collections = createQuery(() => read.collections());
	const counts = $derived(collections.data ? titleCounts(collectionsOf(collections.data)) : null);

	function choose(next: View) {
		view = next;
		kept.set(next === 'titles' ? undefined : next);
	}
</script>

<PageHead title="Library">
	{#snippet sub()}{counts ?? 'Everything on this server'}{/snippet}
	{#snippet end()}
		<div class="views" role="group" aria-label="View">
			<button class="btn" aria-pressed={view === 'titles'} onclick={() => choose('titles')}>Titles</button>
			<button class="btn" aria-pressed={view === 'downloads'} onclick={() => choose('downloads')}>Downloads and seeding</button>
		</div>
	{/snippet}
</PageHead>

{#if view === 'titles'}
	<TitlesView />
{:else}
	<DownloadsView />
{/if}

<style>
	.views {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-1);
	}
	.views .btn {
		min-height: var(--control-h);
	}
</style>
