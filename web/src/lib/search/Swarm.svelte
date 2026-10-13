<script lang="ts">
	// A release's swarm in a search result: its seeders in green, its leechers in red, the counts
	// saying it in words either way.
	import { leechersWords, seedersWords } from './release.ts';

	let { seeders, leechers }: { seeders: number | null | undefined; leechers: number | null | undefined } = $props();
	const seed = $derived(seedersWords(seeders));
	const leech = $derived(leechersWords(leechers));
</script>

{#if seed}<span class="seed">{seed}</span>{/if}{#if seed && leech}{' · '}{/if}{#if leech}<span class="leech">{leech}</span>{/if}

<style>
	.seed {
		color: var(--swarm-seed);
	}
	.leech {
		color: var(--swarm-leech);
	}
</style>
