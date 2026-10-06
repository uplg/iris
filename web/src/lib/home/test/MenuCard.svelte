<script lang="ts">
	// A poster card with its menu, as the home rows draw it (tests only).
	import PosterCard from '#lib/components/PosterCard.svelte';
	import CardMenu from '../CardMenu.svelte';

	let { run }: { run: () => void } = $props();
</script>

<ul>
	<PosterCard href="/collection/c1" title="Andor" art={null} meta="Series">
		{#snippet actions()}
			<CardMenu label="More for Andor" items={[{ label: 'Remove from your watchlist', run }]} />
		{/snippet}
	</PosterCard>
</ul>
