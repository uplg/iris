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
		/** In a dense list: the thumbnail size. */
		small?: boolean;
	}
	let { posterPath, title, gone = false, small = false }: Props = $props();
</script>

<div class="mini" class:gone class:small>
	<Poster src={tmdbImage(posterPath, small ? 'w92' : 'w185')} {title} />
</div>

<style>
	.mini {
		width: var(--poster-mini);
		flex: none;
	}
	.small {
		width: var(--poster-thumb);
	}
	.small :global(.fallback) {
		justify-content: center;
		align-items: center;
		padding: 0;
	}
	.mini :global(.fallback span) {
		display: none;
	}
	.gone {
		opacity: var(--disabled-opacity);
		filter: grayscale(1);
	}
</style>
