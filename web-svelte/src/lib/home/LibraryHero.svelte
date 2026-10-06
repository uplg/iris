<script lang="ts">
	// Nothing to resume: the freshest title of the library (its own verified name and art).
	import type { CollectionListItem } from '@iris/api/client';
	import { formatSize } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import Hero from './Hero.svelte';
	import { kindLabel, stillUrl } from './data.ts';
	import { tmdbMeta } from '#lib/tmdb.svelte.ts';

	let { item }: { item: CollectionListItem } = $props();
	const md = tmdbMeta(() => ({ id: item.tmdb_id, kind: item.kind, trusted: true }));
</script>

<Hero
	eyebrow="In your library"
	title={item.display_title}
	meta={[md.data?.year ? String(md.data.year) : null, kindLabel(item.kind, item.is_anime), formatSize(item.total_size_bytes)]}
	overview={md.data?.overview}
	art={stillUrl(md.data?.backdrop_path, 'w1280')}
>
	{#snippet actions()}
		<a class="btn primary" href="/collection/{item.id}"><Icon name="play" />Open</a>
	{/snippet}
</Hero>
