<script lang="ts">
	// My watch history: everything I watched, finished or not, including what was since
	// reclaimed from disk (listed under its title, never dropped). A gone release is downloaded
	// again in one press and plays from where I stopped (same release, same files, the saved
	// position applies again).
	import { createQuery } from '@tanstack/svelte-query';
	import { goto } from '$app/navigation';
	import { me, torrents } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import PageHead from '#lib/components/PageHead.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import HistoryList from './HistoryList.svelte';
	import { groupHistory, type Item } from './groups.ts';
	import { count } from './words.ts';

	// the server's most; the list stays light at any length (HistoryList)
	const LIMIT = 200;

	const history = createQuery(
		() => ({
			queryKey: ['history'],
			queryFn: () => me.history(LIMIT, 0)
		}),
		() => queryClient
	);
	const value = loadable(history);
	const titles = $derived(groupHistory(history.data ?? []).length);

	async function restore(it: Item) {
		// a past release asked for by name: the duplicate guard does not apply
		await torrents.ingest(it.source_provider!, it.source_external_id!, it.tmdb_id, true);
		void queryClient.invalidateQueries({ queryKey: ['history'] });
		void queryClient.invalidateQueries({ queryKey: ['library'] });
		await goto(`/watch/${it.infohash}/${it.file_idx}`);
	}
</script>

<PageHead title="Watch history">
	{#snippet sub()}{history.data?.length ? count(titles, 'title') : 'What you watched, finished or not.'}{/snippet}
</PageHead>
<Loaded
	{value}
	empty={history.data?.length === 0}
	emptyText="Nothing watched yet."
	emptyHint="What you play shows here, with where you stopped."
>
	<HistoryList items={history.data ?? []} onrestore={restore} />
</Loaded>
