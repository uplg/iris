<script lang="ts" generics="T">
	// A list by day, newest first: each day under its heading (« Today », « Saturday 4 October »),
	// its rows drawn by the caller (a whole `<li>` each). The audit log and the watch history.
	import type { Snippet } from 'svelte';
	import { byDay } from './model.ts';

	interface Props {
		/** Prefixes the day headings' ids. */
		id: string;
		items: readonly T[];
		/** When an item happened (ISO). */
		at: (item: T) => string;
		key: (item: T) => string | number;
		/** When the list was read: the headings count days from it. */
		now: number;
		row: Snippet<[T]>;
	}
	let { id, items, at, key, now, row }: Props = $props();
	const days = $derived(byDay(items, at, now));
</script>

{#each days as day (day.key)}
	<section class="day" aria-labelledby="{id}-{day.key}">
		<h3 class="day-title" id="{id}-{day.key}">{day.heading}</h3>
		<ul class="plain-list">
			{#each day.items as item (key(item))}{@render row(item)}{/each}
		</ul>
	</section>
{/each}

<style>
	.day {
		display: grid;
	}
	.day-title {
		font: var(--t-label);
		font-family: var(--font-text);
		letter-spacing: 0;
		color: var(--ink-muted);
		padding-block: var(--s-2);
		border-bottom: 1px solid var(--line);
	}
</style>
