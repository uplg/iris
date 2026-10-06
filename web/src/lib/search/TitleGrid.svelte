<script lang="ts">
	// The Titles view: what the words could mean on TMDB, each title leading to its releases.
	// Its state comes from what is known: in the library (the server says), and how many of the
	// loaded releases are that title (their `title_match`); nothing is said when nothing is known.
	import type { TitleCard } from '@iris/api/client';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import type { Tone } from '#lib/components/StatusLine.svelte';
	import { kindWord, plural } from '@iris/api/format';

	interface Props {
		titles: readonly TitleCard[];
		counts: ReadonlyMap<number, number>;
		href: (t: TitleCard) => string;
	}
	let { titles, counts, href }: Props = $props();

	function status(t: TitleCard): { tone: Tone; text: string } | undefined {
		const n = counts.get(t.tmdb_id) ?? 0;
		const releases = n ? plural(n, 'release') : '';
		if (t.collection_id) return { tone: 'ok', text: releases ? `In your library · ${releases}` : 'In your library' };
		return releases ? { tone: 'available', text: releases } : undefined;
	}
</script>

<ul class="poster-grid">
	{#each titles as t (`${t.kind}-${t.tmdb_id}`)}
		<PosterCard
			href={href(t)}
			title={t.title}
			art={t.poster_url}
			meta={[kindWord(t.kind), t.year].filter(Boolean).join(' · ')}
			status={status(t)}
		/>
	{/each}
</ul>
