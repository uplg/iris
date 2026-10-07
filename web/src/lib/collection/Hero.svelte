<script lang="ts">
	// The title's banner, one piece: its backdrop (trusted TMDB art only), the poster, the one
	// h1, one line of facts, the story folded to three lines, then what to do: resume where the
	// person stopped (or start), keep it on the watchlist (a series), mark it watched or not.
	// The words never sit on the picture: it fades into the banner's ground beside them (under
	// them on a phone), so they keep their contrast in both themes whatever the art.
	import { follows, library, me, tmdbImage, type CollectionDetail, type ContinueWatchingItem, type TmdbMetadata } from '@iris/api/client';
	import { clock, duration, kindLabel, plural } from '@iris/api/format';
	import { queryClient } from '#lib/query.ts';
	import { KEYS } from '#lib/queries.ts';
	import { allWatched } from '#lib/watched.ts';
	import { refetchCollection } from './actions.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Poster from '#lib/components/Poster.svelte';
	import { firstPlayable, playLabel, type Episode } from './merge.ts';
	import { watchHref } from '#lib/paths.ts';

	interface Props {
		collection: CollectionDetail;
		meta: TmdbMetadata | undefined;
		resume: ContinueWatchingItem | null;
		/** The series' rows (`episodesOf`), merged once by the page. */
		rows: Episode[];
	}
	let { collection: c, meta, resume, rows }: Props = $props();
	const id = $props.id();
	const g = new Gesture();

	const series = $derived(c.kind === 'tv');

	const facts = $derived.by(() => {
		const out: (string | number | null | undefined)[] = [kindLabel(c.kind, c.is_anime), meta?.year];
		if (series) {
			const seasons = new Set(rows.filter((r) => r.absolute === null && r.season > 0).map((r) => r.season)).size;
			if (seasons && c.numbering !== 'absolute') out.push(plural(seasons, 'season'));
			else if (rows.length) out.push(plural(rows.length, 'episode'));
		} else if (meta?.runtime_minutes) out.push(duration(meta.runtime_minutes * 60));
		return out.filter(Boolean).join(' · ');
	});
	const fresh = $derived(c.has_new_since_last_visit ?? 0);

	let noBackdrop = $state(false);
	const backdrop = $derived(noBackdrop ? null : tmdbImage(c.backdrop_path, 'w1280'));

	let expanded = $state(false);
	let clamped = $state(false);
	/** Whether the story overflows its three lines: only then is there anything to unfold. */
	function measure(el: HTMLElement) {
		const ro = new ResizeObserver(() => {
			if (!expanded) clamped = el.scrollHeight > el.clientHeight + 1;
		});
		ro.observe(el);
		return () => ro.disconnect();
	}

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
				void queryClient.invalidateQueries({ queryKey: KEYS.follows });
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

<section class="banner" class:art={!!backdrop} class:framed={!!c.poster_path} aria-labelledby="{id}-title">
	{#if backdrop}
		<img
			class="backdrop"
			src={backdrop}
			alt=""
			width="1280"
			height="720"
			fetchpriority="high"
			decoding="async"
			onerror={() => (noBackdrop = true)}
		/>
	{/if}
	<!-- no poster: the title is right there, the banner needs no stand-in naming it again -->
	{#if c.poster_path}<div class="poster"><Poster src={tmdbImage(c.poster_path, 'w342')} title={c.display_title} eager /></div>{/if}
	<div class="words">
		<h1 id="{id}-title" tabindex="-1">{c.display_title}</h1>
		<p class="facts">{facts}</p>
		{#if fresh > 0}<p class="chip accent fresh">{plural(fresh, 'new episode')} since your last visit</p>{/if}
		{#if meta?.overview}
			<p id="{id}-story" class="overview" class:clamp={!expanded} {@attach measure}>{meta.overview}</p>
			{#if clamped || expanded}
				<button class="link-btn quiet more" aria-expanded={expanded} aria-controls="{id}-story" onclick={() => (expanded = !expanded)}>
					{expanded ? 'Less' : 'More'}
				</button>
			{/if}
		{/if}
		<div class="actions">
			{#if target}
				<a class="btn primary big play" href={watchHref(target.infohash, target.idx)}><Icon name="play" />{label}</a>
			{/if}
			{#if series}
				<button class="btn" aria-pressed={listed} {...pending(g.is('watchlist'))} disabled={listed && !c.normalized_name} onclick={toggle}>
					<Icon name={listed ? 'check' : 'bookmark'} busy={g.is('watchlist')} />On your watchlist
				</button>
			{/if}
			<button class="btn" aria-pressed={watched} {...pending(g.is('watched'))} onclick={toggleWatched}>
				<Icon name={watched ? 'circle-check' : 'circle'} busy={g.is('watched')} />Watched
			</button>
		</div>
	</div>
</section>

<style>
	/* a phone: the picture on top, the poster over its lower edge, the words under them */
	.banner {
		position: relative;
		isolation: isolate;
		display: grid;
		gap: var(--s-4);
		margin-top: var(--s-4);
		padding: var(--s-4);
		border-radius: var(--radius-xl);
		background: var(--ground-raised);
		overflow: hidden;
	}
	.backdrop {
		display: block;
		width: calc(100% + 2 * var(--s-4));
		margin: calc(-1 * var(--s-4)) calc(-1 * var(--s-4)) 0;
		aspect-ratio: 16 / 9;
		height: auto;
		object-fit: cover;
		mask-image: linear-gradient(to bottom, black 55%, transparent);
	}
	.poster {
		width: 6.5rem;
	}
	.art .poster {
		position: relative;
		margin-top: -4.5rem;
	}
	.words {
		display: grid;
		gap: var(--s-3);
		min-width: 0;
		align-content: start;
	}
	h1 {
		font-family: var(--font-display);
		font-size: clamp(2.07rem, 1.77rem + 1.52vw, 3.55rem);
		line-height: 1.05;
		overflow-wrap: anywhere;
		text-wrap: balance;
	}
	.facts {
		margin: 0;
		font: var(--t-secondary);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
	.fresh {
		justify-self: start;
		white-space: normal;
	}
	.overview {
		margin: 0;
		max-width: var(--measure);
		text-wrap: pretty;
	}
	.clamp {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		overflow: hidden;
	}
	.more {
		justify-self: start;
		min-height: var(--control-h);
		margin-block: calc(-1 * var(--s-3));
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2) var(--s-3);
		margin-top: var(--s-2);
	}
	.actions .btn {
		min-height: var(--control-h);
	}
	.actions .play {
		flex: 1 1 100%;
		min-height: var(--control-h-l);
		padding-inline: var(--s-5);
	}
	/* wider: the poster beside the words, the picture on the far side fading into the ground */
	.backdrop {
		grid-area: art;
	}
	.poster {
		grid-area: poster;
	}
	.words {
		grid-area: words;
	}
	.banner {
		grid-template-areas: 'words';
	}
	.banner.art {
		grid-template-areas: 'art' 'words';
	}
	.banner.framed {
		grid-template-areas: 'poster' 'words';
	}
	.banner.art.framed {
		grid-template-areas: 'art' 'poster' 'words';
	}
	@media (min-width: 600px) {
		.banner {
			align-items: end;
			gap: var(--s-5);
			padding: var(--s-5);
		}
		.banner.framed {
			grid-template-columns: auto minmax(0, 1fr);
			grid-template-areas: 'poster words';
		}
		.banner.art.framed {
			grid-template-areas: 'art art' 'poster words';
		}
		.backdrop {
			width: calc(100% + 2 * var(--s-5));
			margin: calc(-1 * var(--s-5)) calc(-1 * var(--s-5)) 0;
			aspect-ratio: 21 / 9;
		}
		.poster {
			width: 9rem;
		}
		.art .poster {
			margin-top: -7rem;
		}
		.actions .play {
			flex: 0 0 auto;
		}
	}
	@media (min-width: 1000px) {
		.banner {
			grid-template-columns: minmax(0, 42rem);
			justify-content: start;
			align-items: center;
			gap: var(--s-6);
			padding: var(--s-6);
		}
		.banner.framed {
			grid-template-columns: auto minmax(0, 42rem);
		}
		.banner.art {
			grid-template-columns: minmax(0, 38rem) minmax(30%, 1fr);
			grid-template-areas: 'words art';
			min-height: 22rem;
		}
		.banner.art.framed {
			grid-template-columns: auto minmax(0, 38rem) minmax(30%, 1fr);
			grid-template-areas: 'poster words art';
		}
		.backdrop {
			align-self: stretch;
			width: calc(100% + var(--s-6));
			height: calc(100% + 2 * var(--s-6));
			margin: calc(-1 * var(--s-6)) calc(-1 * var(--s-6)) calc(-1 * var(--s-6)) 0;
			aspect-ratio: auto;
			mask-image: linear-gradient(to right, transparent, black 60%);
		}
		.poster {
			width: clamp(10rem, 14vw, 13rem);
		}
		.art .poster {
			margin-top: 0;
		}
	}
</style>
