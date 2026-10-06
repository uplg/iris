<script lang="ts">
	// The library: everything on this server, as titles (a poster grid) or as releases
	// (downloading, needing a hand, seeding). The view is remembered per browser.
	import PageHead from '#lib/components/PageHead.svelte';
	import { stored, text } from '#lib/stored.ts';
	import { LIBRARY_VIEW_KEY, VIEWS, type View } from './model.ts';
	import LibraryStats from './LibraryStats.svelte';
	import TitlesView from './TitlesView.svelte';
	import DownloadsView from './DownloadsView.svelte';

	const kept = stored<View>(LIBRARY_VIEW_KEY, 'titles', text(VIEWS));
	let view = $state<View>(kept.get());

	function choose(next: View) {
		view = next;
		kept.set(next === 'titles' ? undefined : next);
	}
</script>

<PageHead title="Library">
	{#snippet sub()}Everything on this server{/snippet}
	{#snippet end()}
		<div class="views" role="group" aria-label="View">
			<button class="btn" aria-pressed={view === 'titles'} onclick={() => choose('titles')}>Titles</button>
			<button class="btn" aria-pressed={view === 'downloads'} onclick={() => choose('downloads')}>Downloads and seeding</button>
		</div>
	{/snippet}
</PageHead>

<LibraryStats />

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
