<script lang="ts">
	// The engine room (admins): the household's accounts, the invitations, who watches now and
	// what was watched, the disk, its upkeep and the log of sensitive actions. Every live
	// figure is read again by its query (refetchInterval), never by a timer of ours.
	import PageHead from '#lib/components/PageHead.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import AuditLog from './AuditLog.svelte';
	import Invitations from './Invitations.svelte';
	import Maintenance from './Maintenance.svelte';
	import NowWatching from './NowWatching.svelte';
	import Storage from './Storage.svelte';
	import Users from './Users.svelte';
	import WatchActivity from './WatchActivity.svelte';
</script>

<PageHead title="Admin">
	{#snippet sub()}People, invitations, playback and the disk.{/snippet}
	{#snippet end()}
		<a class="btn" href="/library?view=torrents"><Icon name="list" />Raw releases view</a>
	{/snippet}
</PageHead>

<!-- what an admin opens the page for first: who watches now and what was watched -->
<div class="sections">
	<div class="pair">
		<NowWatching />
		<WatchActivity />
	</div>
	<div class="pair">
		<Users />
		<Invitations />
	</div>
	<div class="pair">
		<Storage />
		<Maintenance />
	</div>
	<AuditLog />
</div>

<style>
	.sections {
		display: grid;
		gap: var(--s-6);
	}
	.pair {
		display: grid;
		gap: var(--s-6);
	}
	@media (min-width: 1024px) {
		.pair {
			grid-template-columns: repeat(2, minmax(0, 1fr));
			align-items: start;
		}
	}
	.sections :global(.group + .group),
	.pair :global(.group) {
		margin-top: 0;
	}
	.sections :global(.group p) {
		margin: 0;
	}
	/* a narrow screen: a row's actions go under its name, never squeezing it */
	@media (max-width: 599px) {
		.sections :global(.list-row) {
			grid-template-columns: minmax(0, 1fr);
		}
		.sections :global(.list-row > .end) {
			grid-column: 1;
			grid-row: auto;
			justify-content: start;
		}
	}
</style>
