<script lang="ts">
	import { page } from '$app/state';
	import WatchView from '#lib/watch/WatchView.svelte';

	const infohash = $derived(page.params.infohash ?? '');
	const fileIdx = $derived(Number(page.params.idx));
</script>

<!-- one view per file: another episode starts from nothing (position, picks, demotions) -->
{#key `${infohash}:${fileIdx}`}
	{#if infohash && Number.isInteger(fileIdx)}
		<WatchView {infohash} {fileIdx} />
	{:else}
		<div class="empty"><p>This link does not point to a file.</p></div>
	{/if}
{/key}
