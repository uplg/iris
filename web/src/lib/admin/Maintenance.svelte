<script lang="ts">
	// Upkeep (an admin's): freeing disk space now (the clean-up that otherwise runs on its own,
	// asked first: it removes releases), what it removed; and the cache of prepared copies
	// (remuxed for streaming), each deleted to reclaim space or to prepare it afresh on the
	// next play. The cache list is read every 10 s, the ones being prepared then the newest
	// first, its first few until asked for all.
	import { createQuery } from '@tanstack/svelte-query';
	import { admin, type GcReport, type RemuxJobView } from '@iris/api/client';
	import { formatSize, onDay, plural } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import ShowMore from './ShowMore.svelte';
	import { remuxQuery, storageQuery } from './queries.ts';
	import { refreshLibrary } from '#lib/queries.ts';

	/** Copies shown before « Show all ». */
	const FIRST = 6;
	const remux = createQuery(remuxQuery, () => queryClient);
	const value = loadable(remux);
	const g = new Gesture();
	let report = $state.raw<GcReport | null>(null);
	let all = $state(false);
	const copies = $derived(
		(remux.data ?? []).toSorted((a, b) => Number(b.in_flight) - Number(a.in_flight) || (b.mtime ?? 0) - (a.mtime ?? 0))
	);
	const shown = $derived(all ? copies : copies.slice(0, FIRST));
	const cached = $derived(copies.reduce((n, j) => n + j.size_bytes, 0));

	const name = (j: RemuxJobView) =>
		j.torrent_name ? `${j.torrent_name}${typeof j.file_idx === 'number' ? `, file ${j.file_idx + 1}` : ''}` : j.key;
	const stage = (j: RemuxJobView) => (j.in_flight ? 'Being prepared' : j.size_bytes > 0 ? 'Ready' : 'Empty');

	const freeSpace = () =>
		g.run(
			() => admin.triggerGc(),
			(r) => {
				report = r;
				const freed = r.used_bytes_before - r.used_bytes_after;
				ui.toast(`Freed ${formatSize(freed)}: ${plural(r.evicted.length, 'release')} removed.`);
				void queryClient.invalidateQueries({ queryKey: storageQuery().queryKey });
				void refreshLibrary();
			},
			'gc'
		);

	const wipe = (j: RemuxJobView) =>
		g.run(
			() => admin.wipeRemux(j.key),
			async (r) => {
				ui.toast(`Prepared copy deleted: ${formatSize(r.freed_bytes)} freed.`);
				await remux.refetch();
			},
			`wipe:${j.key}`
		);
</script>

<Group id="maintenance-title" title="Maintenance">
	<div class="block">
		<h3 class="label">Disk clean-up</h3>
		<p class="hint">
			Runs on its own when the disk passes the clean-up line. Run it now to remove the oldest finished releases until the disk is back to
			its target.
		</p>
		<div>
			<ConfirmDialog
				icon="trash-2"
				label="Free space now"
				title="Free space now?"
				description="Iris removes the oldest finished releases until the disk is back to its target. Watch history stays, and removed titles can be downloaded again."
				action="Free space"
				busy={g.is('gc')}
				onconfirm={freeSpace}
			/>
		</div>
		{#if report}
			<div class="callout" role="group" aria-labelledby="gc-title">
				<p id="gc-title">
					<strong>Freed {formatSize(report.used_bytes_before - report.used_bytes_after)}</strong> · {plural(
						report.evicted.length,
						'release'
					)} removed
				</p>
				{#if report.evicted.length}
					<ul class="plain-list evicted">
						{#each report.evicted as e (e.infohash)}
							<li><span>{e.name}</span><span class="hint">{formatSize(e.freed_bytes)}</span></li>
						{/each}
					</ul>
				{/if}
			</div>
		{/if}
	</div>

	<div class="block">
		<h3 class="label">
			Prepared copies{#if copies.length}<span class="count"> · {plural(copies.length, 'copy', 'copies')}, {formatSize(cached)}</span>{/if}
		</h3>
		<p class="hint">
			Files rewrapped once for streaming, kept to start playback at once. Delete one to reclaim its space or to prepare it again on the next
			play; the oldest go on their own when the cache is full.
		</p>
		<Loaded {value} empty={copies.length === 0} emptyText="No prepared copies on disk.">
			<ul class="plain-list">
				{#each shown as j (j.key)}
					<ListRow
						second="{stage(j)}{j.size_bytes > 0 ? ` · ${formatSize(j.size_bytes)}` : ''}{j.mtime
							? ` · Updated ${onDay(j.mtime * 1000)}`
							: ''}"
					>
						<span class="name">{name(j)}</span>
						{#snippet end()}
							<button
								class="btn ghost"
								aria-label="Delete the prepared copy of {name(j)}"
								{...pending(g.is(`wipe:${j.key}`))}
								onclick={() => wipe(j)}
							>
								<Icon name="trash-2" busy={g.is(`wipe:${j.key}`)} />Delete
							</button>
						{/snippet}
					</ListRow>
				{/each}
			</ul>
			<ShowMore
				more={!all && copies.length > FIRST}
				label="Show all {plural(copies.length, 'copy', 'copies')}"
				onmore={() => (all = true)}
			/>
		</Loaded>
	</div>
</Group>

<style>
	.block + .block {
		margin-top: var(--s-5);
	}
	.count {
		font-weight: 400;
		color: var(--ink-muted);
	}
	.name {
		overflow-wrap: anywhere;
		min-width: 0;
		font: var(--t-secondary);
	}
	.evicted li {
		display: flex;
		justify-content: space-between;
		gap: var(--s-3);
		overflow-wrap: anywhere;
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
