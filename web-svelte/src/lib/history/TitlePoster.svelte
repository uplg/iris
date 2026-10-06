<script lang="ts">
	// A small poster for a row that knows only its TMDB id (a history row, a session): the
	// artwork looked up once and kept, the shared Poster drawing it (its own fallback when
	// there is none). `gone`: greyed, its row says why in words.
	import { createQuery } from '@tanstack/svelte-query';
	import { metadata, tmdbImage, type MediaKind } from '@iris/api/client';
	import { queryClient } from '#lib/query.ts';
	import Poster from '#lib/components/Poster.svelte';

	interface Props {
		tmdbId: number | null | undefined;
		kind?: MediaKind | null;
		title: string;
		gone?: boolean;
	}
	let { tmdbId, kind, title, gone = false }: Props = $props();

	const art = createQuery(
		() => ({
			queryKey: ['tmdb', tmdbId, kind ?? null],
			queryFn: () => metadata.tmdb(tmdbId!, kind ?? undefined),
			enabled: typeof tmdbId === 'number',
			// TMDB artwork does not change: asked once a visit
			staleTime: Infinity,
			retry: false
		}),
		() => queryClient
	);
</script>

<div class="mini" class:gone>
	<Poster src={tmdbImage(art.data?.poster_path, 'w185')} {title} />
</div>

<style>
	.mini {
		width: var(--poster-mini);
		flex: none;
	}
	.mini :global(.fallback span) {
		display: none;
	}
	.gone {
		opacity: var(--disabled-opacity);
		filter: grayscale(1);
	}
</style>
