<script lang="ts">
	// Four facts over the whole library: titles, disk, downloading, seeding. Each tile shows what
	// is known; one not yet read says so rather than a zero.
	import { formatSize, percent } from '@iris/api/format';
	import Progress from '#lib/components/Progress.svelte';
	import { collectionsOf, downloadingLine, ratioOf, titleCounts } from './model.ts';
	import { collectionsQuery, summaryQuery, torrentsQuery } from './queries.svelte.ts';

	const summary = summaryQuery();
	const collections = collectionsQuery();
	const torrents = torrentsQuery();

	const titles = $derived(collections.data ? titleCounts(collectionsOf(collections.data)) : null);
	const disk = $derived(summary.data?.disk ?? null);
	const seedTotals = $derived(torrents.data?.view === 'torrents' ? torrents.data : null);
	const ratio = $derived(seedTotals ? ratioOf(seedTotals.total_uploaded_bytes, seedTotals.total_downloaded_bytes) : null);
</script>

<section class="stats" aria-labelledby="library-stats-title">
	<h2 id="library-stats-title" class="sr-only">At a glance</h2>
	<dl>
		<div class="tile stat">
			<dt>Titles</dt>
			{#if titles}
				<dd class="value">{titles.total}</dd>
				<dd class="hint">{titles.text}</dd>
			{:else}
				<dd class="hint">{collections.isError ? 'Could not be read' : 'Loading…'}</dd>
			{/if}
		</div>
		<div class="tile stat">
			<dt>Disk</dt>
			{#if disk}
				{@const used = disk.total_bytes - disk.free_bytes}
				<dd class="value">{formatSize(used)} used</dd>
				<dd>
					<Progress
						label="Disk used"
						value={used}
						max={Math.max(1, disk.total_bytes)}
						valueText={percent((used / Math.max(1, disk.total_bytes)) * 100)}
					/>
				</dd>
				<dd class="hint">{formatSize(disk.free_bytes)} free of {formatSize(disk.total_bytes)}</dd>
			{:else}
				<dd class="hint">{summary.isPending ? 'Loading…' : 'Disk space unknown'}</dd>
			{/if}
		</div>
		<div class="tile stat">
			<dt>Downloading</dt>
			{#if summary.data}
				<dd class="value">{summary.data.downloading}</dd>
				<dd class="hint">{downloadingLine(summary.data)}</dd>
			{:else}
				<dd class="hint">{summary.isError ? 'Could not be read' : 'Loading…'}</dd>
			{/if}
		</div>
		<div class="tile stat">
			<dt>Seeding</dt>
			{#if summary.data}
				<dd class="value">{summary.data.seeding}</dd>
				{#if seedTotals}
					<dd class="hint">
						{formatSize(seedTotals.total_uploaded_bytes)} sent in all{#if ratio !== null}{` · ratio ${ratio.toFixed(2)}`}{/if}
					</dd>
				{/if}
			{:else}
				<dd class="hint">{summary.isError ? 'Could not be read' : 'Loading…'}</dd>
			{/if}
		</div>
	</dl>
</section>

<style>
	dl {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(8rem, 100%), 1fr));
		gap: var(--s-3);
		margin: 0 0 var(--s-5);
	}
	.stat {
		gap: var(--s-1);
		align-content: start;
	}
	dt {
		font: var(--t-label);
		color: var(--ink-muted);
	}
	dd {
		margin: 0;
	}
	.value {
		font: var(--t-page);
		font-variant-numeric: tabular-nums;
	}
	.hint {
		font-variant-numeric: tabular-nums;
	}
	/* the bar's own label repeats the tile's: kept for readers only */
	.stat :global(.progress .head) {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0, 0, 0, 0);
		white-space: nowrap;
	}
</style>
