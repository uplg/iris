<script lang="ts">
	// What the household watched lately (an admin's), everyone together, newest first, read
	// again every 30 s: who, what, how far and when, in words.
	import { createQuery } from '@tanstack/svelte-query';
	import type { WatchHistoryEntry } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import Group from '#lib/components/Group.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { fileName, onDay } from '@iris/api/format';
	import { progressWords } from '#lib/history/words.ts';
	import { watchHistoryQuery } from './queries.ts';

	const history = createQuery(watchHistoryQuery, () => queryClient);
	const value = loadable(history);
	const what = (h: WatchHistoryEntry) => fileName(h.file_path) ?? h.torrent_name;
</script>

<Group id="activity-title" title="Watch history">
	<p class="hint">The household’s latest plays. A person’s whole history is under Users.</p>
	<Loaded {value} empty={history.data?.length === 0} emptyText="No playback recorded yet.">
		<!-- a long history scrolls in its own box, so the sections under it stay in reach
		     (browsers make a scroller keyboard-focusable themselves) -->
		<div class="scroll" role="region" aria-label="Latest plays">
			<ul class="plain-list">
				{#each history.data ?? [] as h (`${h.user_id}:${h.infohash}:${h.file_idx}`)}
					<ListRow second="{progressWords(h.position_seconds, h.duration_seconds, h.completed)} · {onDay(h.last_watched_at)}">
						<strong>{h.display_name}</strong>
						<a class="what" href="/watch/{h.infohash}/{h.file_idx}">{what(h)}</a>
					</ListRow>
				{/each}
			</ul>
		</div>
	</Loaded>
</Group>

<style>
	.scroll {
		max-height: 28rem;
		overflow-y: auto;
		overscroll-behavior: contain;
	}
	.what {
		color: var(--ink);
		overflow-wrap: anywhere;
		min-width: 0;
	}
</style>
