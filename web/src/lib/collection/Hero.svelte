<script lang="ts">
	// The title: what it is (kind, year, genres), the one h1, its facts in words (seasons,
	// episodes, rating, releases on disk), the languages and picture of what is on disk, its
	// story, then what to do: resume where the person stopped (or start), keep it on the
	// watchlist (a series), mark it watched or not.
	import { follows, library, me, type CollectionDetail, type ContinueWatchingItem, type TmdbMetadata } from '@iris/api/client';
	import { clock, duration, plural } from '@iris/api/format';
	import { queryClient } from '#lib/query.ts';
	import { KEYS } from '#lib/queries.ts';
	import { allWatched } from '#lib/watched.ts';
	import { refetchCollection } from './actions.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { audioChip, firstPlayable, mergeEpisodes, mergeEpisodesAbsolute, nameLanguage, playLabel, qualityWords } from './merge.ts';
	import { watchHref } from '#lib/paths.ts';

	interface Props {
		collection: CollectionDetail;
		meta: TmdbMetadata | undefined;
		resume: ContinueWatchingItem | null;
	}
	let { collection: c, meta, resume }: Props = $props();
	const g = new Gesture();

	const series = $derived(c.kind === 'tv');
	const eyebrow = $derived(
		[series ? 'Series' : 'Movie', meta?.year, meta?.genres.length ? meta.genres.slice(0, 3).join(', ') : null].filter(Boolean).join(' · ')
	);

	const rows = $derived(
		series
			? c.numbering === 'absolute'
				? mergeEpisodesAbsolute(c.episodes, c.available_episodes, c.gone_episodes)
				: mergeEpisodes(c.episodes, c.available_episodes, c.gone_episodes)
			: []
	);
	const facts = $derived.by(() => {
		const out: string[] = [];
		if (series) {
			const seasons = new Set(rows.filter((r) => r.absolute === null && r.season > 0).map((r) => r.season)).size;
			if (seasons && c.numbering !== 'absolute') out.push(plural(seasons, 'season'));
			if (rows.length) out.push(plural(rows.length, 'episode'));
		} else if (meta?.runtime_minutes) out.push(duration(meta.runtime_minutes * 60));
		if ((meta?.vote_score ?? 0) > 0) out.push(`TMDB ${((meta?.vote_score ?? 0) * 10).toFixed(1)}`);
		out.push(`${plural(c.torrents.length, 'release')} on disk`);
		return out.join(' · ');
	});

	const chips = $derived.by(() => {
		const tags = series ? c.episodes.map((e) => e.language) : c.torrents.map((t) => nameLanguage(t.name ?? ''));
		const audio = [...new Set(tags.filter((t): t is string => !!t))]
			.map((t) => audioChip(t, meta?.original_language))
			.filter((t): t is string => !!t);
		const picture = c.torrents.map((t) => qualityWords(t.name ?? '')).filter((q): q is string => !!q);
		return [...new Set([...audio, ...picture])];
	});
	const fresh = $derived(c.has_new_since_last_visit ?? 0);

	const target = $derived(resume ? { infohash: resume.infohash, idx: resume.file_idx } : firstPlayable(c));
	const label = $derived(playLabel(c, resume, clock));

	const listed = $derived(c.on_watchlist ?? false);
	const watched = $derived(allWatched(c.watch, c.kind, c.episodes.length));

	function toggle() {
		const was = listed;
		void g.run(
			async () => {
				if (was && c.normalized_name) await me.removeFromWatchlist(c.normalized_name);
				else if (!was) await follows.add(c.display_title, c.tmdb_id ?? null);
			},
			async () => {
				void queryClient.invalidateQueries({ queryKey: KEYS.watchlist });
				await refetchCollection(c.id);
				ui.say(was ? `${c.display_title} is no longer on your watchlist.` : `${c.display_title} is on your watchlist.`);
			},
			'watchlist'
		);
	}

	function toggleWatched() {
		const was = watched;
		void g.run(
			() => (was ? library.markUnwatched(c.id) : library.markWatched(c.id)),
			async () => {
				await refetchCollection(c.id);
				ui.say(was ? `${c.display_title} is marked as not watched.` : `${c.display_title} is marked as watched.`);
			},
			'watched'
		);
	}
</script>

<div class="hero-text">
	<p class="eyebrow">{eyebrow}</p>
	<h1 tabindex="-1">{c.display_title}</h1>
	<p class="facts">{facts}</p>
	{#if chips.length || fresh > 0}
		<ul class="plain-list chips" aria-label="Languages and picture">
			{#if fresh > 0}<li class="chip accent">{plural(fresh, 'new episode')} since your last visit</li>{/if}
			{#each chips as chip (chip)}<li class="chip">{chip}</li>{/each}
		</ul>
	{/if}
	{#if meta?.overview}<p class="overview">{meta.overview}</p>{/if}
	<div class="actions">
		{#if target}
			<a class="btn primary big" href={watchHref(target.infohash, target.idx)}><Icon name="play" />{label}</a>
		{/if}
		{#if series}
			<button
				class="btn big"
				aria-pressed={listed}
				{...pending(g.is('watchlist'))}
				disabled={listed && !c.normalized_name}
				onclick={toggle}
			>
				<Icon name={listed ? 'check' : 'bookmark'} busy={g.is('watchlist')} />On your watchlist
			</button>
		{/if}
		<button class="btn big" aria-pressed={watched} {...pending(g.is('watched'))} onclick={toggleWatched}>
			<Icon name={watched ? 'circle-check' : 'circle'} busy={g.is('watched')} />Watched
		</button>
	</div>
</div>

<style>
	.hero-text {
		flex: 1 1 20rem;
		max-width: var(--measure);
		min-width: 0;
		display: grid;
		gap: var(--s-3);
		align-content: start;
	}
	h1 {
		font-family: var(--font-display);
		font-size: clamp(2.07rem, 1.77rem + 1.52vw, 3.55rem);
		line-height: 1.05;
		overflow-wrap: anywhere;
	}
	.facts {
		margin: 0;
		font: var(--t-secondary);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
	.chip {
		white-space: normal;
	}
	.overview {
		margin: 0;
	}
	.actions {
		margin-top: var(--s-2);
	}
	.big {
		min-height: var(--control-h);
		padding-inline: var(--s-4);
	}
</style>
