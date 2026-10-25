<script lang="ts">
	// A film's place on SéléoTV, the household's other app (an admin's): shared for a week, then
	// extended by a week or stopped. Shared, the film stays on disk: the clean-up leaves it
	// alone, and deleting it waits until the share is stopped. What shows is what the server
	// answered (the admin's list of shares, read again after each gesture).
	import { createQuery } from '@tanstack/svelte-query';
	import { admin } from '@iris/api/client';
	import { onDay } from '@iris/api/format';
	import { queryClient } from '#lib/query.ts';
	import { session } from '#lib/session.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { seleoQuery } from '#lib/admin/queries.ts';

	let { infohash, fileIdx, title }: { infohash: string; fileIdx: number; title: string } = $props();

	const WEEK = 7;
	const isAdmin = $derived(!!session.user?.is_admin);
	const shares = createQuery(
		() => ({ ...seleoQuery(), enabled: isAdmin }),
		() => queryClient
	);
	const share = $derived(shares.data?.find((s) => s.infohash === infohash && s.file_idx === fileIdx));
	const g = new Gesture();
	const id = $props.id();

	const day = (iso: string) => onDay(iso).replace(/^on /, '');
	const reread = () => queryClient.invalidateQueries({ queryKey: seleoQuery().queryKey });

	const put = (key: 'share' | 'extend') =>
		g.run(
			() => admin.shareWithSeleo(infohash, fileIdx, WEEK),
			async (s) => {
				await reread();
				ui.toast(`${title} is on SéléoTV until ${day(s.until)}.`);
			},
			key
		);
	const stop = () =>
		g.run(
			() => admin.unshareFromSeleo(infohash, fileIdx),
			async () => {
				await reread();
				ui.toast(`${title} is no longer on SéléoTV.`);
			},
			'stop'
		);
</script>

{#if isAdmin && shares.isSuccess}
	<div class="seleo" role="group" aria-labelledby="{id}-state">
		{#if share}
			<p class="state" id="{id}-state"><Icon name="tv" size={16} />On SéléoTV until {day(share.until)}</p>
			<button class="btn ghost" aria-label="Extend {title} on SéléoTV by a week" {...pending(g.is('extend'))} onclick={() => put('extend')}>
				<Icon name="plus" busy={g.is('extend')} />Extend
			</button>
			<button class="btn ghost" aria-label="Stop sharing {title} with SéléoTV" {...pending(g.is('stop'))} onclick={stop}>
				<Icon name="x" busy={g.is('stop')} />Stop sharing
			</button>
		{:else}
			<p class="sr-only" id="{id}-state">Not on SéléoTV</p>
			<button class="btn ghost" aria-describedby="{id}-why" {...pending(g.is('share'))} onclick={() => put('share')}>
				<Icon name="tv" busy={g.is('share')} />Share with SéléoTV
			</button>
			<span class="hint" id="{id}-why">For a week; it stays on disk meanwhile.</span>
		{/if}
	</div>
{/if}

<style>
	.seleo {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
	}
	.state {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		margin: 0;
		font: var(--t-secondary);
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
