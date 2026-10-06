<script lang="ts">
	// Who changed or deleted what (an admin's), kept by the server, newest first and by day, a
	// page at a time: who, what in words (an action not listed here keeps its own name), when,
	// and what it was about. Narrowed to one kind of action and to one person; read every 30 s.
	import { createInfiniteQuery, createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { clockTime, formatSize, plural } from '@iris/api/format';
	import type { AuditLogEntry } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Select from '#lib/components/Select.svelte';
	import ShowMore from './ShowMore.svelte';
	import { peopleChoices } from './model.ts';
	import DayList from './DayList.svelte';
	import { auditQuery, usersQuery } from './queries.ts';

	const ACTIONS: Record<string, string> = {
		'torrent.delete': 'deleted a release',
		'user.password_reset': 'set a new password',
		'user.password_change': 'changed their password',
		'user.passkeys_revoked': 'removed passkeys',
		'user.display_name_update': 'changed a display name',
		'user.delete': 'deleted an account',
		'gc.evict': 'freed disk space',
		'remux.wipe': 'deleted a prepared copy',
		'provider.enable': 'turned a tracker on',
		'provider.disable': 'turned a tracker off'
	};
	const ALL = 'all';
	/** The families of actions (`user` is every `user.*`). */
	const FAMILIES = [
		{ value: ALL, label: 'Everything' },
		{ value: 'user', label: 'Accounts' },
		{ value: 'torrent', label: 'Releases' },
		{ value: 'gc', label: 'Disk clean-ups' },
		{ value: 'remux', label: 'Prepared copies' },
		{ value: 'provider', label: 'Trackers' }
	];

	let family = $state(ALL);
	let actor = $state(ALL);
	const filter = $derived({ action: family === ALL ? undefined : family, actor_id: actor === ALL ? undefined : actor });
	const filtered = $derived(family !== ALL || actor !== ALL);
	const log = createInfiniteQuery(
		() => ({ ...auditQuery(filter), placeholderData: keepPreviousData }),
		() => queryClient
	);
	const users = createQuery(usersQuery, () => queryClient);
	const value = loadable(log);
	const rows = $derived(log.data?.pages.flat() ?? []);
	const people = $derived(peopleChoices(users.data, { value: ALL, label: 'Anyone' }));

	/** The details as people read them: byte counts as sizes, releases counted in words. */
	const details = (e: AuditLogEntry) =>
		e.details
			?.replace(/(\d+) bytes/g, (_, n: string) => formatSize(Number(n)))
			.replace(/(\d+) torrent\(s\) evicted/g, (_, n: string) => `${plural(Number(n), 'release')} removed`);

	async function more() {
		const before = rows.length;
		await log.fetchNextPage();
		const added = rows.length - before;
		if (added > 0) ui.say(`${plural(added, 'more entry', 'more entries')} shown.`);
	}
</script>

<Group id="audit-title" title="Audit log" fact={rows.length ? 'Newest first' : undefined}>
	<p class="hint">Deletions, password changes, clean-ups and trackers turned on or off, and who made them.</p>
	<div class="filters">
		<div class="pick"><Select label="What" value={family} options={FAMILIES} onchange={(v) => (family = v)} /></div>
		<div class="pick"><Select label="Who" value={actor} options={people} onchange={(v) => (actor = v)} /></div>
	</div>
	<Loaded {value} empty={rows.length === 0} emptyText={filtered ? 'Nothing recorded matches these filters.' : 'Nothing recorded yet.'}>
		<div class="days">
			<DayList id="audit" items={rows} at={(e) => e.created_at} key={(e) => e.id} now={log.dataUpdatedAt || Date.now()}>
				{#snippet row(e)}
					<li class="entry">
						<time datetime={e.created_at}>{clockTime(e.created_at)}</time>
						<div class="text">
							<span><strong>{e.actor_display_name}</strong> {ACTIONS[e.action] ?? e.action}</span>
							{#if details(e)}<span class="details">{details(e)}</span>{/if}
						</div>
					</li>
				{/snippet}
			</DayList>
		</div>
		<ShowMore
			more={!!log.hasNextPage}
			busy={log.isFetchingNextPage}
			label="Show more entries"
			done={(log.data?.pages.length ?? 0) > 1 ? `That’s everything: ${plural(rows.length, 'entry', 'entries')}.` : undefined}
			onmore={more}
		/>
	</Loaded>
</Group>

<style>
	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-3) var(--s-4);
	}
	.pick {
		flex: 0 1 15rem;
		min-width: min(12rem, 100%);
	}
	.days {
		display: grid;
		gap: var(--s-4);
		max-width: var(--measure-wide);
	}
	.entry {
		display: grid;
		grid-template-columns: 3.5rem minmax(0, 1fr);
		gap: var(--s-3);
		align-items: baseline;
		min-height: var(--row-min);
		padding-block: var(--s-2);
		border-bottom: 1px solid var(--line);
		content-visibility: auto;
		contain-intrinsic-size: auto 3.5rem;
	}
	time {
		font: var(--t-meta);
		color: var(--ink-muted);
		font-variant-numeric: tabular-nums;
	}
	.text {
		display: grid;
		gap: var(--s-half);
		min-width: 0;
	}
	.details {
		font: var(--t-secondary);
		color: var(--ink-muted);
		overflow-wrap: anywhere;
	}
</style>
