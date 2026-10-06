<script lang="ts">
	// A title of the library: its poster (the server's), what it is, and whether it is on disk,
	// still downloading, or reclaimed (its history kept, its files gone).
	import { tmdbImage, type CollectionListItem } from '@iris/api/client';
	import { formatSize, kindLabel, percent, plural } from '@iris/api/format';
	import PosterCard from '#lib/components/PosterCard.svelte';

	let { item, downloading }: { item: CollectionListItem; downloading?: number } = $props();

	const meta = $derived(
		`${kindLabel(item.kind, item.is_anime)} · ${item.kind === 'tv' ? plural(item.episode_count, 'episode') : formatSize(item.total_size_bytes)}`
	);
	const status = $derived.by(() => {
		if (item.ghost) return { tone: 'warn' as const, text: 'No longer on disk' };
		if (downloading !== undefined) return { tone: 'busy' as const, text: `Downloading · ${percent(downloading)}` };
		return { tone: 'ok' as const, text: 'On disk' };
	});
</script>

<PosterCard href="/collection/{item.id}" title={item.display_title} art={tmdbImage(item.poster_path, 'w342')} {meta} {status} />
