<script lang="ts">
	// A brand-new, empty library: the tracker's freshest featured release. Its name comes from
	// the server's title match when there is one, else the release name tidied.
	import type { SearchResult } from '@iris/api/client';
	import { prettySceneName } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import Hero from './Hero.svelte';
	import { kindLabel, plural, stillUrl } from './data.ts';
	import { tmdbMeta } from './tmdb.svelte.ts';

	let { result }: { result: SearchResult } = $props();
	const match = $derived(result.title_match ?? null);
	// the server's title match first: an indexer's own tmdb id is often wrong
	const md = tmdbMeta(() => ({ id: match?.tmdb_id ?? result.tmdb_id, kind: match?.kind ?? result.kind, trusted: true }));
	const title = $derived(md.data?.title ?? match?.title ?? prettySceneName(result.title));
</script>

<Hero
	eyebrow="Featured"
	{title}
	meta={[
		(result.year ?? match?.year) ? String(result.year ?? match?.year) : null,
		kindLabel(result.kind ?? match?.kind),
		typeof result.seeders === 'number' ? plural(result.seeders, 'seeder') : null
	]}
	overview={md.data?.overview}
	art={stillUrl(md.data?.backdrop_path, 'w1280')}
>
	{#snippet actions()}
		<a class="btn primary" href="/search?q={encodeURIComponent(title)}"><Icon name="search" />Find releases</a>
	{/snippet}
</Hero>
