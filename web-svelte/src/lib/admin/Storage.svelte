<script lang="ts">
	// The disk (an admin's), read every 10 s: how full, against the line where clean-up starts
	// and the level it cleans down to, said in words (past the line, a warning in words too),
	// and what the household gave back to the swarm.
	import { createQuery } from '@tanstack/svelte-query';
	import { formatSize, percent } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import Group from '#lib/components/Group.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Progress from '#lib/components/Progress.svelte';
	import StatusRow from '#lib/components/StatusRow.svelte';
	import { count } from '#lib/history/words.ts';
	import { storageQuery } from './queries.ts';

	const storage = createQuery(storageQuery, () => queryClient);
	const value = loadable(storage);
	const s = $derived(storage.data);
	const used = $derived(s && s.max_storage_bytes > 0 ? Math.min(100, (s.used_bytes / s.max_storage_bytes) * 100) : 0);
	const over = $derived(!!s && s.used_bytes >= s.threshold_bytes);
	// lifetime upload over lifetime download (both kept after a release is removed)
	const ratio = $derived(s && (s.total_downloaded_bytes ?? 0) > 0 ? s.total_uploaded_bytes / s.total_downloaded_bytes! : null);
</script>

<Group id="storage-title" title="Storage">
	<Loaded {value}>
		{#if s}
			<Progress
				label="Disk used"
				value={used}
				max={100}
				valueText="{formatSize(s.used_bytes)} of {formatSize(s.max_storage_bytes)} ({percent(used)})"
			/>
			<dl class="facts">
				<StatusRow
					label="State"
					value={over ? 'Past the clean-up line: the next clean-up frees space' : 'Below the clean-up line'}
					warn={over}
				/>
				<StatusRow label="On disk" value={count(s.torrent_count, 'release')} />
				<StatusRow label="Clean-up starts at" value="{s.threshold_pct}% ({formatSize(s.threshold_bytes)})" />
				<StatusRow label="Cleans down to" value="{s.target_pct}% ({formatSize(s.target_bytes)})" />
				<StatusRow
					label="Shared back, all time"
					value="{formatSize(s.total_uploaded_bytes)}{ratio === null ? '' : `, ratio ${ratio.toFixed(2)}`}"
				/>
			</dl>
		{/if}
	</Loaded>
</Group>
