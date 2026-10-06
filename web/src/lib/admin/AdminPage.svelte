<script lang="ts">
	// The engine room (admins): one line of what is going on now, then four views, one at a
	// time: the activity (who watches now, what was watched), the people (accounts and
	// invitations), the system (the disk, the upkeep, the trackers) and the log of sensitive
	// actions. The view is a tab, kept in the URL (`?view=`, without a navigation: the focus
	// stays on the tab) and remembered per browser; only the shown view reads its server values.
	// Every live figure is read again by its query (refetchInterval), never by a timer of ours.
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import PageHead from '#lib/components/PageHead.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Tabs from '#lib/components/Tabs.svelte';
	import { refocus } from '#lib/focus.ts';
	import { STORAGE } from '#lib/storage.ts';
	import { stored, text } from '#lib/stored.ts';
	import AuditLog from './AuditLog.svelte';
	import Glance from './Glance.svelte';
	import Invitations from './Invitations.svelte';
	import Maintenance from './Maintenance.svelte';
	import NowWatching from './NowWatching.svelte';
	import PlayHistory from './PlayHistory.svelte';
	import Storage from './Storage.svelte';
	import Trackers from './Trackers.svelte';
	import Users from './Users.svelte';
	import { isView, VIEWS, type View } from './model.ts';

	const TABS = [
		{ value: 'activity', label: 'Activity', icon: 'play' },
		{ value: 'people', label: 'People', icon: 'users' },
		{ value: 'system', label: 'System', icon: 'hard-drive' },
		{ value: 'log', label: 'Log', icon: 'list' }
	] as const;

	const kept = stored<View>(STORAGE.adminView, VIEWS[0], text(VIEWS));
	const asked = untrack(() => page.url.searchParams.get('view'));
	let view = $state<View>(isView(asked) ? asked : kept.get());

	function choose(next: View) {
		view = next;
		kept.set(next === VIEWS[0] ? undefined : next);
		const url = new URL(page.url.href);
		url.searchParams.set('view', next);
		replaceState(url, {});
	}

	async function open(next: View, section: string) {
		choose(next);
		await refocus(section);
	}
</script>

<PageHead title="Admin">
	{#snippet end()}
		<a class="btn" href="/library?view=torrents"><Icon name="list" />Raw releases view</a>
	{/snippet}
</PageHead>
<Glance onopen={open} />

<div class="views">
	<Tabs tabs={TABS} value={view} onchange={(v) => isView(v) && choose(v)}>
		{#snippet panel(v)}
			{#if v === view}
				{#if v === 'activity'}
					<div class="activity">
						<NowWatching />
						<PlayHistory />
					</div>
				{:else if v === 'people'}
					<div class="pair">
						<Users />
						<Invitations />
					</div>
				{:else if v === 'system'}
					<div class="pair">
						<div class="stack inner">
							<Storage />
							<Maintenance />
						</div>
						<Trackers />
					</div>
				{:else}
					<AuditLog />
				{/if}
			{/if}
		{/snippet}
	</Tabs>
</div>

<style>
	/* a phone: the four tabs fit by their words alone, never wider than the screen */
	@media (max-width: 599px) {
		.views :global(.tabs-list) {
			gap: 0;
		}
		.views :global(.tabs-trigger) {
			flex: 1 1 0;
			justify-content: center;
			padding-inline: var(--s-1);
			min-width: 0;
		}
		.views :global(.tabs-trigger .icon) {
			display: none;
		}
	}
	.stack,
	.pair,
	.activity {
		display: grid;
		gap: var(--s-6);
		align-items: start;
		padding-top: var(--s-2);
	}
	.inner {
		padding-top: 0;
	}
	@media (min-width: 1024px) {
		.pair {
			grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
			column-gap: var(--s-7);
		}
	}
	.stack :global(.group + .group),
	.activity :global(.group),
	.pair :global(.group) {
		margin-top: 0;
	}
	.pair :global(.group p),
	.activity :global(.group p),
	.stack :global(.group p) {
		margin: 0;
	}
</style>
