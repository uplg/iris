<script lang="ts">
	// The disk (an admin's), read every 10 s: one bar, how full and how much is free, the line
	// where clean-up starts marked on it and said in words (past it, a warning in words too),
	// then what is on it and what the household gave back to the swarm.
	import { createQuery } from '@tanstack/svelte-query';
	import { formatSize, percent, plural } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import StatusRow from '#lib/components/StatusRow.svelte';
	import { storageQuery } from './queries.ts';

	const storage = createQuery(storageQuery, () => queryClient);
	const value = loadable(storage);
	const s = $derived(storage.data);
	const used = $derived(s && s.max_storage_bytes > 0 ? Math.min(100, (s.used_bytes / s.max_storage_bytes) * 100) : 0);
	const free = $derived(s ? Math.max(0, s.max_storage_bytes - s.used_bytes) : 0);
	const over = $derived(!!s && s.used_bytes >= s.threshold_bytes);
	// lifetime upload over lifetime download (both kept after a release is removed)
	const ratio = $derived(s && (s.total_downloaded_bytes ?? 0) > 0 ? s.total_uploaded_bytes / s.total_downloaded_bytes! : null);
	const words = $derived(
		s ? `${formatSize(s.used_bytes)} of ${formatSize(s.max_storage_bytes)} used (${percent(used)}), ${formatSize(free)} free` : ''
	);
</script>

<Group id="storage-title" title="Disk">
	<Loaded {value}>
		{#if s}
			<div class="disk">
				<p class="used" id="disk-words">{words}</p>
				<div
					class="bar"
					class:over
					role="meter"
					aria-labelledby="disk-words"
					aria-valuemin={0}
					aria-valuemax={100}
					aria-valuenow={Math.round(used)}
					aria-valuetext={words}
				>
					<span class="fill" style:width="{used}%"></span>
					<span class="line" style:left="{s.threshold_pct}%" aria-hidden="true"></span>
				</div>
				<p class={['state', over && 'warn-text']}>
					{#if over}<Icon name="triangle-alert" size={16} />Past the clean-up line ({s.threshold_pct}%): the next clean-up frees space down
						to
						{s.target_pct}%.
					{:else}Clean-up starts at {s.threshold_pct}% ({formatSize(s.threshold_bytes)}) and frees space down to {s.target_pct}%.{/if}
				</p>
			</div>
			<dl class="facts">
				<StatusRow label="On disk" value={plural(s.torrent_count, 'release')} />
				<StatusRow
					label="Shared back, all time"
					value="{formatSize(s.total_uploaded_bytes)}{ratio === null ? '' : `, ratio ${ratio.toFixed(2)}`}"
				/>
			</dl>
		{/if}
	</Loaded>
</Group>

<style>
	.disk {
		display: grid;
		gap: var(--s-2);
	}
	.used {
		margin: 0;
		font-variant-numeric: tabular-nums;
	}
	.bar {
		position: relative;
		height: var(--bar-thin);
		border-radius: var(--radius-pill);
		background: var(--ground-raised);
	}
	.fill {
		position: absolute;
		inset: 0 auto 0 0;
		border-radius: var(--radius-pill);
		background: var(--accent);
	}
	.over .fill {
		background: var(--status-degraded);
	}
	/* the clean-up line: a notch through the bar, its place said in words under it */
	.line {
		position: absolute;
		top: calc(-1 * var(--s-1));
		bottom: calc(-1 * var(--s-1));
		width: var(--s-half);
		margin-left: calc(-1 * var(--s-half) / 2);
		background: var(--ink);
		border-radius: var(--radius-xs);
	}
	.state {
		margin: 0;
		font: var(--t-secondary);
		color: var(--ink-muted);
		display: flex;
		gap: var(--s-1);
		align-items: baseline;
	}
	.state.warn-text {
		color: var(--warn-text);
		font-weight: 600;
	}
	.state :global(.icon) {
		flex: none;
		align-self: center;
	}
</style>
