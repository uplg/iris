<script lang="ts">
	// Discover: tonight's moods (a board of tiles, then one mood's titles), movies or series,
	// then every suggestion shelf. The mood and the kind live in the address
	// (`?mood=&kind=`): shareable, and Back goes from a mood to the board. Switching the kind
	// rewrites the address in place (no navigation: the focus stays on the switch).
	import { afterNavigate, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import type { MediaKind } from '@iris/api/client';
	import PageHead from '#lib/components/PageHead.svelte';
	import ToggleGroup from '#lib/components/ToggleGroup.svelte';
	import { ui } from '#lib/ui.svelte.ts';
	import MoodBoard from '#lib/discover/MoodBoard.svelte';
	import MoodResults from '#lib/discover/MoodResults.svelte';
	import ForYou from '#lib/discover/ForYou.svelte';

	const KINDS = [
		{ value: 'movie', label: 'Movies' },
		{ value: 'tv', label: 'Series' }
	] as const;

	const fromUrl = $derived<MediaKind>(page.url.searchParams.get('kind') === 'tv' ? 'tv' : 'movie');
	const mood = $derived(page.url.searchParams.get('mood'));
	let chosen = $state<MediaKind | null>(null);
	const kind = $derived(chosen ?? fromUrl);
	afterNavigate(() => (chosen = null));

	function setKind(next: MediaKind) {
		chosen = next;
		const url = new URL(page.url.href);
		url.searchParams.set('kind', next);
		replaceState(url, page.state);
		ui.say(next === 'tv' ? 'Showing series' : 'Showing movies');
	}
</script>

<PageHead title="Discover">
	{#snippet sub()}Tonight's moods and what is trending, checked against your trackers.{/snippet}
</PageHead>

<div class="discover">
	<section class="tonight" aria-labelledby="tonight-title">
		<div class="head">
			<h2 id="tonight-title" class="group-title">What are you in the mood for?</h2>
			<ToggleGroup type="single" label="Show" hideLabel options={KINDS} value={kind} onchange={setKind} />
		</div>
		{#if mood}
			{#key `${mood}:${kind}`}<MoodResults {mood} {kind} />{/key}
		{:else}
			<MoodBoard {kind} />
		{/if}
	</section>

	<ForYou />
</div>

<style>
	.discover {
		display: grid;
		gap: var(--s-6);
		min-width: 0;
	}
	.tonight {
		display: grid;
		gap: var(--s-4);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-3);
	}
</style>
