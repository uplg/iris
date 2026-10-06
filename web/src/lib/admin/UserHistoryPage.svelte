<script lang="ts">
	// One person's watch history, for an admin: the same list as their own History page, read
	// only (nothing is downloaded again from here).
	import { createQuery } from '@tanstack/svelte-query';
	import { admin } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import PageHead from '#lib/components/PageHead.svelte';
	import HistoryList from '#lib/history/HistoryList.svelte';
	import { usersQuery } from './queries.ts';

	let { userId }: { userId: string } = $props();
	const LIMIT = 200;

	const history = createQuery(
		() => ({
			queryKey: ['admin', 'user-history', userId],
			queryFn: () => admin.userHistory(userId, LIMIT, 0)
		}),
		() => queryClient
	);
	const users = createQuery(usersQuery, () => queryClient);
	const who = $derived(users.data?.find((u) => u.id === userId));
	const value = loadable(history);
</script>

<a class="back link-btn quiet" href="/admin"><Icon name="arrow-left" />Admin</a>
<PageHead title={who ? `${who.display_name}’s history` : 'Watch history'}>
	{#snippet sub()}{who ? `Everything ${who.display_name} watched, finished or not.` : 'Everything this person watched.'}{/snippet}
</PageHead>
<Loaded {value} empty={history.data?.length === 0} emptyText="Nothing watched yet." missing="This account no longer exists.">
	<HistoryList items={history.data ?? []} collections={false} />
</Loaded>

<style>
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		min-height: var(--control-h);
		margin-top: var(--s-3);
	}
</style>
