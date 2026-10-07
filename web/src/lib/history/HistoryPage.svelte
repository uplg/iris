<script lang="ts">
	// My watch history: everything I watched, finished or not, including what was since
	// reclaimed from disk (listed under its title, never dropped). A gone release is downloaded
	// again in one press and plays from where I stopped (same release, same files, the saved
	// position applies again).
	import { createQuery } from '@tanstack/svelte-query';
	import { plural } from '@iris/api/format';
	import { goto } from '$app/navigation';
	import { me } from '@iris/api/client';
	import { fetchAgainOrSearch } from '#lib/regrab.ts';
	import { loadable, queryClient } from '#lib/query.ts';
	import { KEYS } from '#lib/queries.ts';
	import PageHead from '#lib/components/PageHead.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import HistoryList from './HistoryList.svelte';
	import { groupHistory, type Item } from './groups.ts';

	// the server's most; the list stays light at any length (HistoryList)
	const LIMIT = 200;

	const history = createQuery(
		() => ({
			queryKey: KEYS.history,
			queryFn: () => me.history(LIMIT, 0)
		}),
		() => queryClient
	);
	const value = loadable(history);
	const groups = $derived(groupHistory(history.data ?? []));

	async function restore(it: Item) {
		const episode = typeof it.season === 'number' && typeof it.episode === 'number' ? { season: it.season, episode: it.episode } : null;
		const res = await fetchAgainOrSearch({ infohash: it.infohash, title: it.collection_title, episode, name: it.torrent_name });
		// a refusal went to Search
		if (res) await goto(`/watch/${it.infohash}/${it.file_idx}`);
	}
</script>

<PageHead title="Watch history">
	{#snippet sub()}{history.data?.length ? plural(groups.length, 'title') : 'What you watched, finished or not.'}{/snippet}
</PageHead>
<Loaded
	{value}
	empty={history.data?.length === 0}
	emptyText="Nothing watched yet."
	emptyHint="What you play shows here, with where you stopped."
>
	<HistoryList {groups} onrestore={restore} />
</Loaded>
