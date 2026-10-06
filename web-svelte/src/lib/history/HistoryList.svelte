<script lang="ts">
	// A watch history (groups.ts): a series under its title, each episode watched under it; a
	// film on one line. Each line: what it is (the link to play it), how far one got and when,
	// in words. What is gone from disk says so and, when its release is known, can be
	// downloaded again (`onrestore`). Long histories stay light: rows out of view are not laid
	// out (content-visibility), yet all are in the page for find-in-page and screen readers.
	import { pending, Gesture } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import TitlePoster from './TitlePoster.svelte';
	import { canRestore, groupHistory, itemKey, type Group, type Item } from './groups.ts';
	import { onDay, progressWords, watchedShare, whatWatched } from './words.ts';

	interface Props {
		items: readonly Item[];
		/** Download a gone release again; without it the list only reads (an admin's view). */
		onrestore?: (it: Item) => Promise<unknown>;
		/** Titles lead to their collection page. */
		collections?: boolean;
	}
	let { items, onrestore, collections = true }: Props = $props();
	const groups = $derived(groupHistory(items));
	const g = new Gesture();

	const watchHref = (it: Item) => `/watch/${it.infohash}/${it.file_idx}`;
	const label = (group: Group, it: Item) => (group.solo ? group.title : (whatWatched(it) ?? it.torrent_name));
	const facts = (it: Item) =>
		`${progressWords(it.position_seconds, it.duration_seconds, it.completed)} · Last watched ${onDay(it.last_watched_at)}`;
	const restore = (it: Item) => onrestore && g.run(() => onrestore(it), undefined, itemKey(it));
</script>

{#snippet line(group: Group, it: Item)}
	{@const name = label(group, it)}
	<div class="line">
		<div class="text">
			<span class="name">
				{#if !it.deleted}
					<a href={watchHref(it)} aria-label="Play {group.solo ? name : `${group.title}, ${name}`}">{name}</a>
				{:else if group.solo && collections && group.collectionId}
					<a href="/collection/{group.collectionId}">{name}</a>
				{:else}
					{name}
				{/if}
				{#if it.deleted}<span class="chip"><Icon name="ban" size={12} />Gone from disk</span>{/if}
			</span>
			<span class="meta">{facts(it)}</span>
			<span class="bar" aria-hidden="true"
				><span style:width="{Math.round(watchedShare(it.position_seconds, it.duration_seconds, it.completed) * 100)}%"></span></span
			>
		</div>
		{#if onrestore && canRestore(it)}
			<button
				class="btn"
				aria-label="Download {group.solo ? name : `${group.title}, ${name}`} again"
				{...pending(g.is(itemKey(it)))}
				onclick={() => restore(it)}
			>
				<Icon name="rotate-ccw" busy={g.is(itemKey(it))} />Download again
			</button>
		{/if}
	</div>
{/snippet}

<ul class="plain-list history">
	{#each groups as group (group.key)}
		<li class="title-row">
			<TitlePoster tmdbId={group.tmdbId} kind={group.kind} title={group.title} gone={group.ghost} />
			{#if group.solo}
				{@render line(group, group.items[0])}
			{:else}
				<div class="series">
					<h2 class="series-title">
						{#if collections && group.collectionId}
							<a href="/collection/{group.collectionId}">{group.title}</a>
						{:else}
							{group.title}
						{/if}
						{#if group.ghost}<span class="chip"><Icon name="ban" size={12} />Gone from disk</span>{/if}
					</h2>
					<ul class="plain-list episodes">
						{#each group.items as it (itemKey(it))}
							<li>{@render line(group, it)}</li>
						{/each}
					</ul>
				</div>
			{/if}
		</li>
	{/each}
</ul>

<style>
	.history {
		display: grid;
		max-width: var(--measure-wide);
	}
	.title-row {
		display: flex;
		gap: var(--s-3);
		align-items: flex-start;
		padding-block: var(--s-3);
		border-bottom: 1px solid var(--line);
		/* out of view, a row is not laid out nor painted: a long history scrolls light */
		content-visibility: auto;
		contain-intrinsic-size: auto 9rem;
	}
	.series {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: var(--s-1);
	}
	.series-title {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
		font: var(--t-body);
		font-family: var(--font-text);
		font-weight: 600;
		letter-spacing: 0;
	}
	.episodes {
		display: grid;
	}
	.episodes > li + li {
		border-top: 1px solid var(--line);
	}
	.line {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2) var(--s-3);
		min-height: var(--row-min);
		padding-block: var(--s-1);
	}
	.text {
		flex: 1 1 14rem;
		min-width: 0;
		display: grid;
		gap: var(--s-1);
	}
	.name {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
		overflow-wrap: anywhere;
	}
	.name a,
	.series-title a {
		color: var(--ink);
		font-weight: 600;
		min-height: var(--control-h-xs);
		display: inline-flex;
		align-items: center;
	}
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
		font-variant-numeric: tabular-nums;
	}
	.bar {
		height: var(--track-h);
		max-width: 16rem;
		border-radius: var(--radius-pill);
		background: var(--ground-raised);
		overflow: hidden;
	}
	.bar span {
		display: block;
		height: 100%;
		background: var(--accent);
	}
	.line .btn {
		min-height: var(--control-h);
	}
</style>
