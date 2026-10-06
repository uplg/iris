<script lang="ts">
	// Search: the words asked (on submit, never per key), what the library already holds first,
	// then the trackers' releases as TMDB titles, posters or a list, filtered by kind, ordered,
	// and narrowed to an audio language among what is loaded. The address carries the search
	// (params.ts): a reload or a shared link replays it.
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery } from '@tanstack/svelte-query';
	import { me, search, type AggregatedResults, type MediaKind, type SearchResult, type TmdbSuggestion } from '@iris/api/client';
	import { LANGUAGE_TAGS, languageLabel, type LanguageTag } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import PageHead from '#lib/components/PageHead.svelte';
	import ToggleGroup from '#lib/components/ToggleGroup.svelte';
	import { SEARCH_KEY, rememberSearch } from './cache.ts';
	import { KINDS, SORT_MODES, readSearch, searchHref, searchOpts, type SearchState, type SortMode } from './params.ts';
	import { failedTrackers, releaseKey, releasesByTitle, summary } from './release.ts';
	import { VIEWS, keptView, type ResultsView } from './view.ts';
	import LibraryMatches from './LibraryMatches.svelte';
	import PillGroup from './PillGroup.svelte';
	import RecentSearches from './RecentSearches.svelte';
	import ReleaseCard from './ReleaseCard.svelte';
	import ReleaseRow from './ReleaseRow.svelte';
	import SearchField from './SearchField.svelte';
	import TitleGrid from './TitleGrid.svelte';
	import TrackerAlerts from './TrackerAlerts.svelte';

	const PAGE_SIZE = 25;
	const id = $props.id();

	const opened = readSearch(page.url);
	let s = $state<SearchState>(opened);
	let field = $state(opened.q);
	let view = $state<ResultsView>(keptView.get());
	let tooShort = $state(false);
	let input = $state<HTMLInputElement>();
	let heading = $state<HTMLElement>();
	const more = new Gesture();

	// a real navigation to /search (a link, the header) starts from its own address
	let read = page.url.href;
	$effect(() => {
		if (page.url.href === read) return;
		read = page.url.href;
		s = readSearch(page.url);
		field = s.q;
	});

	// the address follows the search, in place: a filter is not a new page, and a navigation
	// would take the focus from the control just pressed to the page's title
	let synced = false;
	$effect(() => {
		const href = searchHref(s);
		rememberSearch(href);
		if (!synced) return void (synced = true);
		if (href !== location.pathname + location.search) replaceState(href, {});
	});

	function nextPage(last: AggregatedResults, loaded: number): number | undefined {
		const pages = Math.max(0, ...last.providers.map((p) => p.total_pages ?? 0));
		if (pages > 0) return loaded < pages ? loaded + 1 : undefined;
		return last.results.length >= PAGE_SIZE ? loaded + 1 : undefined;
	}

	const results = createInfiniteQuery(
		() => {
			const asked = { ...s, lang: null };
			return {
				queryKey: [SEARCH_KEY, asked.q, asked.kind, asked.sort, asked.title],
				// a newer search aborts this one (its signal): the tracker fan-out stops server-side
				queryFn: ({ pageParam, signal }: { pageParam: number; signal: AbortSignal }) =>
					search.query(asked.q, searchOpts(asked, pageParam, PAGE_SIZE), signal),
				initialPageParam: 1,
				getNextPageParam: (last: AggregatedResults, all: AggregatedResults[]) => nextPage(last, all.length),
				enabled: asked.q.length >= 2
			};
		},
		() => queryClient
	);

	const effective = $derived<ResultsView>(s.title && view === 'titles' ? 'list' : view);
	const titles = createQuery(
		() => {
			const q = s.q;
			return {
				queryKey: ['search-titles', q],
				queryFn: ({ signal }: { signal: AbortSignal }) => search.titles(q, signal),
				enabled: q.length >= 2 && (effective === 'titles' || s.title !== null),
				staleTime: 5 * 60_000
			};
		},
		() => queryClient
	);

	const pages = $derived(results.data?.pages ?? []);
	// a tracker's page boundary can move between two asks (new uploads): page 2 may repeat the
	// end of page 1, and a release is shown once
	const rows = $derived.by(() => {
		const seen = new Set<string>();
		const out: SearchResult[] = [];
		for (const p of pages)
			for (const r of p.results) {
				const k = releaseKey(r);
				if (!seen.has(k)) {
					seen.add(k);
					out.push(r);
				}
			}
		return out;
	});
	const meta = $derived(pages.at(-1)?.providers ?? []);
	const parsed = $derived(pages[0]?.parsed_query ?? null);
	const matches = $derived((pages[0]?.library_matches ?? []).filter((m) => s.title === null || m.tmdb_id === s.title));
	const langCounts = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const r of rows) if (r.language_tag) counts.set(r.language_tag, (counts.get(r.language_tag) ?? 0) + 1);
		return counts;
	});
	const shown = $derived(s.lang ? rows.filter((r) => r.language_tag === s.lang) : rows);
	const counts = $derived(releasesByTitle(shown));
	const titleCards = $derived((titles.data ?? []).filter((t) => !s.kind || t.kind === s.kind));
	const unmatched = $derived(shown.filter((r) => !r.title_match).length);
	const chosen = $derived.by(() => {
		if (s.title === null) return null;
		const card = titles.data?.find((t) => t.tmdb_id === s.title);
		if (card) return card.title;
		return rows.find((r) => r.title_match?.tmdb_id === s.title)?.title_match?.title ?? null;
	});

	const settled = $derived(results.isSuccess && !results.isFetching);
	const line = $derived(settled ? summary(matches.length, shown.length, meta) : '');
	let said = '';
	$effect(() => {
		if (line && line !== said) ui.say((said = line));
	});

	const audioOptions = $derived([
		{ value: null as LanguageTag | null, label: 'Any language', count: rows.length },
		...LANGUAGE_TAGS.filter((t) => langCounts.has(t.tag) || t.tag === s.lang).map((t) => ({
			value: t.tag as LanguageTag | null,
			label: t.short,
			count: langCounts.get(t.tag) ?? 0
		}))
	]);

	/** How many releases the next page should bring: each tracker still paging gives its limit. */
	const nextCount = $derived.by(() => {
		const total = meta.reduce((n, p) => n + (p.total_count ?? 0), 0);
		const next = meta.filter((p) => !p.error && (!p.total_pages || p.current_page < p.total_pages)).reduce((n, p) => n + p.limit, 0);
		return total > rows.length ? Math.min(next, total - rows.length) : next;
	});

	function submit(q: string) {
		tooShort = q.length > 0 && q.length < 2;
		if (tooShort) return;
		field = q;
		if (q === s.q) {
			if (q) void results.refetch();
		} else s = { ...s, q, lang: null, title: null };
		if (!q) return;
		// the recent searches are a convenience: failing to keep one is not the search's failure
		me.recordSearch(q).then(
			() => queryClient.invalidateQueries({ queryKey: ['recent-searches'] }),
			() => undefined
		);
	}

	function pick(t: TmdbSuggestion) {
		s = { ...s, kind: t.kind };
		submit(t.title.trim());
	}

	const setKind = (kind: MediaKind | null) => (s = { ...s, kind, lang: null });
	const setSort = (sort: SortMode) => (s = { ...s, sort });
	const setLang = (lang: LanguageTag | null) => (s = { ...s, lang });

	function setView(v: ResultsView) {
		view = v;
		keptView.set(v);
		if (v === 'titles' && s.title !== null) s = { ...s, title: null };
	}

	function showMore() {
		return more.run(
			() => results.fetchNextPage({ throwOnError: true }),
			() => (results.hasNextPage ? undefined : refocus(heading)),
			'more'
		);
	}

	const titleHref = (tmdb: number) => searchHref({ ...s, lang: null, title: tmdb });
</script>

<PageHead title="Search" />

<div class="search-page">
	<SearchField bind:value={field} bind:input onsubmit={submit} onpick={pick} describedby={parsed ? `${id}-parsed` : undefined} />
	{#if tooShort}<p class="form-error">Type at least 2 characters.</p>{/if}
	{#if parsed && s.q}
		<p class="hint" id="{id}-parsed">
			Showing results for <strong>{parsed.title}</strong>{#if typeof parsed.season === 'number'}{' · '}{parsed.episode
					? `Season ${parsed.season}, episode ${parsed.episode}`
					: `Season ${parsed.season}`}{/if}{#if typeof parsed.year === 'number'}{' · '}{parsed.year}{/if}.
		</p>
	{/if}

	<RecentSearches onsearch={submit} back={() => input} />

	<div class="filters">
		<PillGroup legend="Type" options={KINDS} value={s.kind} onchange={setKind} />
		<PillGroup legend="Sort" options={SORT_MODES} value={s.sort} onchange={setSort} />
		{#if rows.length || s.lang}
			<PillGroup legend="Audio" options={audioOptions} value={s.lang} onchange={setLang} />
		{/if}
	</div>

	{#if s.q.length < 2}
		<div class="empty">
			<p>Search for a movie or a series.</p>
			<p class="hint">
				Type a title, a year or a release name, then press Search. Picking a title from the suggestions gives the trackers its exact name.
			</p>
		</div>
	{:else}
		<Loaded value={loadable(results)}>
			{#if line}<p class="summary">{line}</p>{/if}
			<TrackerAlerts failed={failedTrackers(meta)} retry={() => results.refetch()} />

			{#if matches.length}<LibraryMatches {matches} />{/if}

			<section class="results" aria-labelledby="{id}-results">
				<div class="results-head">
					<h2 id="{id}-results" class="group-title" tabindex="-1" bind:this={heading}>
						{chosen ? `Releases of ${chosen}` : effective === 'titles' ? 'Titles' : 'Releases'}
					</h2>
					<ToggleGroup type="single" label="Show as" hideLabel options={VIEWS} value={effective} onchange={setView} />
				</div>
				{#if s.title !== null}
					<a class="link-btn back" href={searchHref({ ...s, lang: null, title: null })}><Icon name="arrow-left" size={16} />All titles</a>
				{/if}

				{#if effective === 'titles'}
					<Loaded
						value={loadable(titles)}
						empty={titleCards.length === 0}
						emptyText="TMDB knows no title for these words."
						emptyHint="Grid and List show every release found."
					>
						<TitleGrid titles={titleCards} {counts} href={(t) => titleHref(t.tmdb_id)} />
					</Loaded>
					{#if unmatched}
						<p class="hint">
							{unmatched}
							{unmatched === 1 ? 'release matches' : 'releases match'} no title.
							<button class="link-btn" onclick={() => setView('list')}>See every release in the list</button>
						</p>
					{/if}
				{:else if shown.length === 0}
					<div class="empty">
						{#if s.lang && rows.length}
							<p>No release in {languageLabel(s.lang, 'short')} among the {rows.length} loaded.</p>
							<p class="hint">The audio filter only sees the loaded releases: load more, or choose another language.</p>
						{:else if !matches.length}
							<p>Nothing found for “{s.q}”.</p>
							<p class="hint">Try another spelling, drop the year, or choose Movies or Series.</p>
						{:else}
							<p>No tracker has a release for “{s.q}”.</p>
						{/if}
					</div>
				{:else if effective === 'grid'}
					<ul class="grid plain-list">
						{#each shown as r (releaseKey(r))}<ReleaseCard {r} />{/each}
					</ul>
				{:else}
					<ul class="plain-list">
						{#each shown as r (releaseKey(r))}<ReleaseRow {r} />{/each}
					</ul>
				{/if}

				{#if results.hasNextPage && effective !== 'titles'}
					<button class="btn more" {...pending(more.is('more'))} onclick={showMore}>
						<Icon name="chevron-down" busy={more.is('more')} />{nextCount
							? `Show ${nextCount} more ${nextCount === 1 ? 'release' : 'releases'}`
							: 'Show more releases'}
					</button>
				{/if}
			</section>
		</Loaded>
	{/if}
</div>

<style>
	.search-page {
		display: grid;
		gap: var(--s-5);
		min-width: 0;
	}
	.filters {
		display: grid;
		gap: var(--s-4);
	}
	.summary {
		margin: 0 0 var(--s-4);
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.results {
		display: grid;
		gap: var(--s-4);
		margin-top: var(--s-5);
		min-width: 0;
	}
	.results-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-3);
	}
	.results-head :global(.choices) {
		min-width: min(18rem, 100%);
	}
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		min-height: var(--control-h);
		justify-self: start;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(10rem, 40vw), 1fr));
		gap: var(--s-5) var(--s-4);
	}
	.more {
		justify-self: center;
		min-height: var(--control-h);
	}
</style>
