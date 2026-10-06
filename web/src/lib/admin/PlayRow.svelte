<script lang="ts">
	// One play of the household's history: what it was as people name it (the title, its
	// episode or year; the release's name only when nothing better is known), who watched, how
	// far, and when. Wide, a row of columns; narrow, the same facts stacked under the title.
	import type { WatchHistoryEntry } from '@iris/api/client';
	import { ago, clockTime } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import Meter from '#lib/components/Meter.svelte';
	import TitlePoster from '#lib/history/TitlePoster.svelte';
	import { playName, progressShort, watchedShare } from '#lib/history/words.ts';
	import { personHref } from '#lib/paths.ts';

	interface Props {
		play: WatchHistoryEntry;
		/** When the list was read: « 12 min ago » counts from it. */
		now: number;
		/** Say who watched (not on that person's own page). */
		person?: boolean;
	}
	let { play, now, person = true }: Props = $props();
	const name = $derived(playName(play));
	const recent = $derived(now - new Date(play.last_watched_at).getTime() < 3_600_000);
</script>

<li class="play">
	<TitlePoster small posterPath={play.poster_path} title={name.title} gone={play.deleted} />
	<div class="what">
		<span class="title">
			{#if play.collection_id}
				<a href="/collection/{play.collection_id}">{name.title}</a>
			{:else}
				<span>{name.title}</span>
			{/if}
			{#if play.deleted}<span class="chip"><Icon name="ban" size={12} />Gone from disk</span>{/if}
		</span>
		{#if name.detail}<span class="detail">{name.detail}</span>{/if}
	</div>
	<div class="facts">
		{#if person}<a class="who" href={personHref(play.user_id)}>{play.display_name}</a>{/if}
		<span class="how">
			<span class="words">
				{#if play.completed}<Icon name="circle-check" size={14} />{/if}{progressShort(
					play.position_seconds,
					play.duration_seconds,
					play.completed
				)}
			</span>
			{#if !play.completed}<Meter thin share={watchedShare(play.position_seconds, play.duration_seconds)} />{/if}
		</span>
	</div>
	<time class="when" datetime={play.last_watched_at}
		>{recent ? ago(play.last_watched_at, 'sentence', now) : clockTime(play.last_watched_at)}</time
	>
</li>

<style>
	.play {
		display: grid;
		grid-template-columns: var(--poster-thumb) minmax(0, 1fr) auto;
		grid-template-areas:
			'art what when'
			'art facts facts';
		gap: var(--s-1) var(--s-3);
		align-items: start;
		padding-block: var(--s-2);
		border-bottom: 1px solid var(--line);
	}
	.play > :global(.mini) {
		grid-area: art;
	}
	.what {
		grid-area: what;
		display: grid;
		gap: var(--s-half);
		min-width: 0;
	}
	.title {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-1) var(--s-2);
		font-weight: 600;
		overflow-wrap: anywhere;
	}
	.title a,
	.who {
		color: var(--ink);
		text-decoration: none;
		min-height: var(--control-h-xs);
		display: inline-flex;
		align-items: center;
	}
	.title a:hover,
	.who:hover {
		text-decoration: underline;
	}
	.detail {
		font: var(--t-secondary);
		color: var(--ink-muted);
		overflow-wrap: anywhere;
	}
	.facts {
		grid-area: facts;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-1) var(--s-3);
		min-width: 0;
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.who {
		font: var(--t-label);
	}
	.how {
		display: grid;
		gap: var(--s-1);
		flex: 1 1 10rem;
		min-width: 0;
		max-width: 16rem;
	}
	.words {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		font-variant-numeric: tabular-nums;
	}
	.words :global(.icon) {
		color: var(--accent);
	}
	.when {
		grid-area: when;
		font: var(--t-meta);
		color: var(--ink-muted);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		line-height: var(--control-h-xs);
	}
	/* a wide list: one line of columns, the person, how far and when aligned down the list */
	@container plays (min-width: 46rem) {
		.play {
			grid-template-columns: var(--poster-thumb) minmax(0, 1fr) minmax(0, 24rem) 6.5rem;
			grid-template-areas: 'art what facts when';
			align-items: center;
		}
		.facts {
			display: grid;
			grid-template-columns: minmax(0, 9rem) minmax(0, 1fr);
			gap: var(--s-4);
		}
		.facts:not(:has(.who)) {
			grid-template-columns: minmax(0, 1fr);
		}
		.how {
			max-width: none;
		}
		.when {
			text-align: right;
		}
	}
</style>
