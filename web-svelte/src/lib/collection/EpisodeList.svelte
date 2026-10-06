<script lang="ts">
	// An ordered list of episodes drawn a window at a time: a thousand rows (One Piece) are not
	// all mounted, each with its own reads. The window opens near where the person is (the
	// first episode not watched) and grows as its end nears the screen (IntersectionObserver,
	// no timer); two buttons do the same for the keyboard. Rows outside the screen are not laid
	// out either (`content-visibility`, EpisodeRow).
	import { untrack } from 'svelte';
	import { plural } from '@iris/api/format';
	import type { TorrentView } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';
	import EpisodeRow from './EpisodeRow.svelte';
	import { watchedEp, type Episode } from './merge.ts';

	interface Props {
		collectionId: string;
		episodes: Episode[];
		torrents: Map<string, TorrentView>;
		/** Rows drawn at once, and added at each step. */
		step?: number;
	}
	let { collectionId, episodes, torrents, step = 40 }: Props = $props();

	const lead = 3;
	const opening = untrack(() => {
		// a short list opens whole; a long one at the first episode not watched
		if (episodes.length <= step) return 0;
		const at = episodes.findIndex((e) => !watchedEp(e));
		return at <= lead ? 0 : at - lead;
	});
	let start = $state(opening);
	let end = $state(untrack(() => opening + step));
	const shown = $derived(episodes.slice(start, end));
	const after = $derived(Math.max(0, episodes.length - end));
	let more = $state<HTMLElement>();

	$effect(() => {
		if (!more) return;
		void end;
		// observed anew after each step: still in view, it fires again at once
		const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && grow(), { rootMargin: '0px 0px 600px 0px' });
		io.observe(more);
		return () => io.disconnect();
	});

	function grow() {
		end = Math.min(episodes.length, end + step);
	}
</script>

{#if start > 0}
	<button class="btn wide" onclick={() => (start = Math.max(0, start - step))}>
		<Icon name="chevron-up" />Show {plural(Math.min(step, start), 'earlier episode')}
	</button>
{/if}
<ol class="episodes" aria-label={plural(episodes.length, 'episode')}>
	{#each shown as ep (ep.absolute !== null ? `a${ep.absolute}` : `${ep.season}-${ep.episode}`)}
		<EpisodeRow {collectionId} {ep} {torrents} />
	{/each}
</ol>
{#if after > 0}
	<button class="btn wide" bind:this={more} onclick={grow}>
		<Icon name="chevron-down" />Show {plural(Math.min(step, after), 'more episode')} ({after} left)
	</button>
{/if}

<style>
	.episodes {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.wide {
		width: 100%;
		justify-content: center;
		min-height: var(--control-h);
	}
</style>
