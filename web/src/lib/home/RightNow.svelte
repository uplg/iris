<script lang="ts">
	// "Right now": what the house's Iris is doing, in a few words (downloads, new episodes,
	// room on disk, what it shares). Read again every few seconds while something downloads.
	import { createQuery } from '@tanstack/svelte-query';
	import Loaded from '#lib/components/Loaded.svelte';
	import { loadable } from '#lib/query.ts';
	import { read } from '#lib/queries.ts';
	import { rightNow } from './data.ts';

	const summary = createQuery(() => read.summary());
	const facts = $derived(summary.data ? rightNow(summary.data) : []);
	const value = loadable(summary);
</script>

{#if value.loading || value.failed || facts.length}
	<section class="now" aria-labelledby="now-title">
		<h2 id="now-title" class="eyebrow">Right now</h2>
		<Loaded {value}>
			<ul class="facts">
				{#each facts as fact (fact)}<li>{fact}</li>{/each}
			</ul>
		</Loaded>
	</section>
{/if}

<style>
	.now {
		display: grid;
		gap: var(--s-2);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.facts li {
		display: inline-flex;
		align-items: center;
		min-height: var(--control-h-xs);
		padding: 0 var(--s-3);
		border: 1px solid var(--line);
		border-radius: var(--radius-pill);
		font: var(--t-secondary);
		font-variant-numeric: tabular-nums;
		color: var(--ink);
	}
</style>
