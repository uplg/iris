<script lang="ts">
	// A brand-new, empty library: the tracker's freshest featured release. Its name comes from
	// the server's title match when there is one, else the release name tidied.
	import { tmdbImage, type SearchResult } from '@iris/api/client';
	import { kindLabel, prettySceneName } from '@iris/api/format';
	import { seedersWords } from '#lib/search/release.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Hero from './Hero.svelte';
	import { tmdbMeta } from '#lib/tmdb.svelte.ts';

	let { result }: { result: SearchResult } = $props();
	const match = $derived(result.title_match ?? null);
	// only the server's title match, set when it trusts it: the indexer's raw id is never shown
	const md = tmdbMeta(() => ({ id: match?.tmdb_id, kind: match?.kind, trusted: match !== null }));
	const title = $derived(md.data?.title ?? match?.title ?? prettySceneName(result.title));
</script>

<Hero
	eyebrow="Featured"
	{title}
	meta={[
		(result.year ?? match?.year) ? String(result.year ?? match?.year) : null,
		kindLabel(result.kind ?? match?.kind),
		seedersWords(result.seeders)
	]}
	overview={md.data?.overview}
	art={tmdbImage(md.data?.backdrop_path, 'w1280')}
>
	{#snippet actions()}
		<a class="btn primary" href="/search?q={encodeURIComponent(title)}"><Icon name="search" />Find releases</a>
	{/snippet}
</Hero>
