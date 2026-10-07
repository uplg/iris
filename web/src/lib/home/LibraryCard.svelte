<script lang="ts">
	// A title of the library: its poster (the server's), what it is, and a line only when it is
	// still downloading or reclaimed (its history kept, its files gone). Being on disk goes
	// without saying.
	import { tmdbImage, type CollectionListItem } from '@iris/api/client';
	import { kindLabel, percent, plural } from '@iris/api/format';
	import PosterCard from '#lib/components/PosterCard.svelte';

	let { item, downloading }: { item: CollectionListItem; downloading?: number } = $props();

	// what it is, and for a series how many episodes; its size on disk says nothing here
	const meta = $derived(
		[kindLabel(item.kind, item.is_anime), item.kind === 'tv' && item.episode_count > 0 ? plural(item.episode_count, 'episode') : null]
			.filter(Boolean)
			.join(' · ')
	);
	const status = $derived.by(() => {
		if (item.ghost) return { tone: 'warn' as const, text: 'No longer on disk' };
		if (downloading !== undefined) return { tone: 'busy' as const, text: `Downloading · ${percent(downloading)}` };
		return undefined;
	});
</script>

<PosterCard href="/collection/{item.id}" title={item.display_title} art={tmdbImage(item.poster_path, 'w342')} {meta} {status} />
