<script lang="ts">
	// One episode: its number, its name (a link when it plays), its state in words, and one
	// action per release: play what is on disk, grab and play an offer in its language, download
	// again what was reclaimed (or hide it). Every action is named with the episode.
	import Meter from '#lib/components/Meter.svelte';
	import { goto } from '$app/navigation';
	import { library, me, type TorrentView } from '@iris/api/client';
	import { fetchAgain } from '#lib/regrab.ts';
	import { formatSize } from '@iris/api/format';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus, sectionHeading } from '#lib/focus.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import { refetchCollection } from './actions.ts';
	import { episodeName, episodeWords, languageWord, type Available, type Downloaded, type Episode, type Gone } from './merge.ts';
	import { downloading, offersByLanguage, rowState, type Verb } from './status.ts';
	import { watchHref } from '#lib/paths.ts';

	interface Props {
		collectionId: string;
		ep: Episode;
		torrents: Map<string, TorrentView>;
	}
	let { collectionId, ep, torrents }: Props = $props();

	const g = new Gesture();
	let row = $state<HTMLLIElement>();

	const disk = $derived(ep.variants.filter((v): v is Downloaded => v.status === 'downloaded'));
	const gone = $derived(ep.variants.filter((v): v is Gone => v.status === 'gone'));
	const offers = $derived(offersByLanguage(ep));
	// several languages on one row: each action says which one it plays
	const several = $derived(new Set(ep.variants.map((v) => v.language ?? '')).size > 1);

	const first = $derived(disk[0]);
	const now = $derived(rowState(ep, { torrent: (ih) => torrents.get(ih) }));

	const what = $derived(episodeWords(ep));
	const inLanguage = (lang: string | null) => {
		const w = several ? languageWord(lang) : null;
		return w ? ` in ${w}` : '';
	};
	function verbOf(v: Downloaded, i: number): Verb {
		if (i === 0 && now.verb) return now.verb;
		if (downloading(torrents.get(v.infohash))) return 'Play while downloading';
		return v.watched ? 'Watch again' : 'Play';
	}
	const offerFacts = (o: Available) =>
		[
			languageWord(o.language) ?? 'Unknown language',
			o.quality,
			o.seeders !== null ? `${o.seeders} seeders` : null,
			o.size_bytes !== null ? formatSize(o.size_bytes) : null
		]
			.filter(Boolean)
			.join(' · ');

	/** After the row changed under the focus: its first action, else the next row's, else the list's title. */
	function focusAfter(next: Element | null | undefined) {
		return refocus(
			() => (row?.isConnected ? row.querySelector<HTMLElement>('.actions a, .actions button') : null),
			() => next?.querySelector<HTMLElement>('.actions a, .actions button'),
			sectionHeading(row)
		);
	}

	function grab(o: Available) {
		const key = `grab:${o.language ?? ''}`;
		void g.run(
			() => library.grabCollectionEpisode(collectionId, ep.season, ep.episode, o.language),
			async (res) => {
				await refetchCollection(collectionId);
				await goto(watchHref(res.infohash, res.file_idx));
			},
			key
		);
	}

	function again(v: Gone) {
		void g.run(
			() => fetchAgain(v.infohash),
			async () => {
				await refetchCollection(collectionId);
				await goto(watchHref(v.infohash, v.file_idx));
			},
			`again:${v.infohash}`
		);
	}

	function hide(v: Gone) {
		const next = row?.nextElementSibling ?? row?.previousElementSibling;
		void g.run(
			() => me.dismissGone({ infohash: v.infohash }),
			async () => {
				await refetchCollection(collectionId);
				ui.say(`The removed release of ${what} is hidden. Your history is kept.`);
				await focusAfter(next);
			},
			`hide:${v.infohash}`
		);
	}
</script>

<li class="ep" bind:this={row}>
	<span class="num" aria-hidden="true">{ep.absolute ?? ep.episode}</span>
	<div class="body">
		<h3 class="name">
			{#if first}<a href={watchHref(first.infohash, first.file_idx)}>{episodeName(ep)}</a>{:else}{episodeName(ep)}{/if}
		</h3>
		<StatusLine tone={now.tone} text={now.text} />
		{#if now.progress !== undefined}
			<Meter share={now.progress} --meter-max="var(--pane)" />
		{/if}
		{#if offers.length}
			<p class="hint">{offers.map((o) => offerFacts(o[0])).join('; ')}</p>
		{/if}
		{#if gone.length}
			<p class="hint">
				{gone
					.map((v) =>
						['Removed', languageWord(v.language), v.quality, v.total_size_bytes > 0 ? formatSize(v.total_size_bytes) : null]
							.filter(Boolean)
							.join(' · ')
					)
					.join('; ')}
			</p>
		{/if}
		<div class="actions">
			{#each disk as v, i (`${v.infohash}:${v.file_idx}`)}
				{@const verb = verbOf(v, i)}
				<a
					class={['btn', i === 0 && 'primary']}
					href={watchHref(v.infohash, v.file_idx)}
					aria-label="{verb}{inLanguage(v.language)}: {what}"
				>
					<Icon name="play" size={16} />{verb}{inLanguage(v.language)}
				</a>
			{/each}
			{#each offers as group (group[0].language ?? '')}
				{@const o = group[0]}
				{@const key = `grab:${o.language ?? ''}`}
				<button
					class={['btn', !disk.length && o === offers[0][0] && 'primary']}
					aria-label="Grab and play{inLanguage(o.language)}: {what}"
					{...pending(g.is(key))}
					onclick={() => grab(o)}
				>
					<Icon name="download" size={16} busy={g.is(key)} />Grab and play{inLanguage(o.language)}
				</button>
			{/each}
			{#each gone as v (v.infohash)}
				<button
					class="btn"
					aria-label="Download again{inLanguage(v.language)}: {what}"
					{...pending(g.is(`again:${v.infohash}`))}
					onclick={() => again(v)}
				>
					<Icon name="rotate-ccw" size={16} busy={g.is(`again:${v.infohash}`)} />Download again{inLanguage(v.language)}
				</button>
				<button
					class="btn ghost"
					aria-label="Hide the removed release{inLanguage(v.language)}: {what}"
					{...pending(g.is(`hide:${v.infohash}`))}
					onclick={() => hide(v)}
				>
					<Icon name="x" size={16} busy={g.is(`hide:${v.infohash}`)} />Hide
				</button>
			{/each}
		</div>
	</div>
</li>

<style>
	.ep {
		display: grid;
		grid-template-columns: var(--control-h) minmax(0, 1fr);
		gap: var(--s-3);
		align-items: start;
		padding: var(--s-3) 0;
		border-bottom: 1px solid var(--line);
		/* a long list (One Piece) draws only what is near the screen */
		content-visibility: auto;
		contain-intrinsic-size: auto 8rem;
	}
	.num {
		display: grid;
		place-items: center;
		width: var(--control-h);
		height: var(--control-h);
		border-radius: var(--radius-m);
		background: var(--ground-raised);
		font: var(--t-group);
		font-variant-numeric: tabular-nums;
	}
	.body {
		display: grid;
		gap: var(--s-2);
		min-width: 0;
	}
	.name {
		font: var(--t-body);
		font-weight: 600;
		font-family: var(--font-text);
	}
	.name a {
		color: var(--ink);
		text-decoration: none;
	}
	.name a:hover {
		text-decoration: underline;
		text-underline-offset: var(--s-half);
	}
	.hint {
		overflow-wrap: anywhere;
	}
	.actions .btn {
		min-height: var(--control-h);
	}
</style>
