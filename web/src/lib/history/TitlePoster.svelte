<script lang="ts">
	// A small poster for a history row or a session: the server's TMDB poster (sent only once the
	// match is verified), the shared Poster drawing it (its own fallback when there is none).
	// `gone`: greyed, its row says why in words.
	import { tmdbImage } from '@iris/api/client';
	import Poster from '#lib/components/Poster.svelte';

	interface Props {
		posterPath: string | null | undefined;
		title: string;
		gone?: boolean;
	}
	let { posterPath, title, gone = false }: Props = $props();
</script>

<div class="mini" class:gone>
	<Poster src={tmdbImage(posterPath, 'w185')} {title} />
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
