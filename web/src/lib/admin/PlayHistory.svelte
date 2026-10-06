<script lang="ts">
	// What the household watched, newest first, by day: a page of plays, then more on a press.
	// Narrowed to one person (a filter, or the person's own page: `userId`) and to series or
	// films. Read again every 30 s (the query's refetchInterval).
	import { createInfiniteQuery, createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import type { MediaKind } from '@iris/api/client';
	import { plural } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Select from '#lib/components/Select.svelte';
	import ToggleGroup from '#lib/components/ToggleGroup.svelte';
	import PlayRow from './PlayRow.svelte';
	import ShowMore from './ShowMore.svelte';
	import { byDay, peopleChoices } from './model.ts';
	import { playsQuery, usersQuery } from './queries.ts';

	interface Props {
		/** One person's plays only (their page): no person filter, nor their name on each row. */
		userId?: string;
	}
	let { userId }: Props = $props();

	const EVERYONE = 'everyone';
	type Kind = 'all' | MediaKind;
	const KINDS: { value: Kind; label: string }[] = [
		{ value: 'all', label: 'Everything' },
		{ value: 'tv', label: 'Series' },
		{ value: 'movie', label: 'Films' }
	];

	let who = $state(EVERYONE);
	let kind = $state<Kind>('all');
	const filter = $derived({
		user_id: userId ?? (who === EVERYONE ? undefined : who),
		kind: kind === 'all' ? undefined : kind
	});
	const filtered = $derived(filter.kind !== undefined || (!userId && filter.user_id !== undefined));

	const plays = createInfiniteQuery(
		() => ({ ...playsQuery(filter), placeholderData: keepPreviousData }),
		() => queryClient
	);
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(plays);
	const rows = $derived(plays.data?.pages.flat() ?? []);
	const now = $derived(plays.dataUpdatedAt || Date.now());
	const days = $derived(byDay(rows, (p) => p.last_watched_at, now));
	const people = $derived(peopleChoices(users.data, { value: EVERYONE, label: 'Everyone' }));

	async function more() {
		const before = rows.length;
		await plays.fetchNextPage();
		const added = rows.length - before;
		if (added > 0) ui.say(`${plural(added, 'more play')} shown.`);
	}

	function clear() {
		who = EVERYONE;
		kind = 'all';
	}
</script>

<Group id="plays-title" title={userId ? 'What they watched' : 'Watch history'} fact={rows.length ? 'Newest first' : undefined}>
	<div class="filters">
		{#if !userId}
			<div class="person">
				<Select label="Person" value={who} options={people} onchange={(v) => (who = v)} />
			</div>
		{/if}
		<ToggleGroup type="single" label="Kind" options={KINDS} value={kind} onchange={(v) => (kind = v)} />
	</div>
	<Loaded {value} empty={rows.length === 0} emptyText={filtered ? 'No plays match these filters.' : 'No playback recorded yet.'}>
		<div class={['plays', userId && 'narrow']}>
			{#each days as day (day.key)}
				<section class="day" aria-labelledby="plays-{day.key}">
					<h3 class="day-title" id="plays-{day.key}">{day.heading}</h3>
					<ul class="plain-list">
						{#each day.items as p (`${p.user_id}:${p.infohash}:${p.file_idx}`)}
							<PlayRow play={p} {now} person={!userId} />
						{/each}
					</ul>
				</section>
			{/each}
		</div>
		<ShowMore
			more={!!plays.hasNextPage}
			busy={plays.isFetchingNextPage}
			label="Show more plays"
			done={(plays.data?.pages.length ?? 0) > 1 ? `That’s everything: ${plural(rows.length, 'play')}.` : undefined}
			onmore={more}
		/>
	</Loaded>
	{#if filtered && rows.length === 0 && !plays.isPending}
		<div><button class="btn" onclick={clear}>Clear the filters</button></div>
	{/if}
</Group>

<style>
	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-3) var(--s-4);
		align-items: end;
	}
	.person {
		flex: 0 1 16rem;
		min-width: min(12rem, 100%);
	}
	.filters :global(.choices) {
		flex: 0 1 20rem;
	}
	.plays {
		container: plays / inline-size;
		display: grid;
		gap: var(--s-4);
	}
	/* one person's list has no person column: kept to a readable width */
	.narrow {
		max-width: var(--measure-wide);
	}
	.day {
		display: grid;
	}
	.day-title {
		font: var(--t-label);
		font-family: var(--font-text);
		color: var(--ink-muted);
		padding-block: var(--s-2);
		border-bottom: 1px solid var(--line);
		letter-spacing: 0;
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
