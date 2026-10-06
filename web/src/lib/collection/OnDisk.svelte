<script lang="ts">
	// The releases on disk for this title: what each one is, who added it, how far its download
	// is; a movie's copies play from here. Deleting is for an admin or whoever added it (the
	// server says so per release, `can_delete`): others see the control, not operable, and why.
	import { torrents as torrentsApi, type CollectionDetail, type TorrentView } from '@iris/api/client';
	import { formatRecentTime, formatSize, percent, plural } from '@iris/api/format';
	import { Gesture, unavailable } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import { refetchCollection } from './actions.ts';
	import { mainVideo, qualityWords, watchHref } from './merge.ts';
	import { downloading, eta } from './status.ts';

	let { collection: c }: { collection: CollectionDetail } = $props();
	const id = $props.id();
	const g = new Gesture();
	let heading = $state<HTMLElement>();
	let list = $state<HTMLElement>();

	const NO_DELETE = 'Only an admin, or the person who added it, can delete this.';
	const nameOf = (t: TorrentView) => t.name ?? t.infohash;
	const facts = (t: TorrentView) =>
		[qualityWords(nameOf(t)), formatSize(t.total_size_bytes), `added by ${t.added_by_name} ${formatRecentTime(t.added_at)}`]
			.filter(Boolean)
			.join(' · ');

	function remove(t: TorrentView) {
		const row = list?.querySelector(`[data-infohash="${t.infohash}"]`);
		const next = row?.nextElementSibling ?? row?.previousElementSibling;
		void g.run(
			() => torrentsApi.remove(t.infohash),
			async () => {
				await refetchCollection(c.id);
				ui.toast(`Deleted ${nameOf(t)}.`);
				await refocus(() => next?.querySelector<HTMLElement>('a, button'), heading, 'main h1');
			},
			`delete:${t.infohash}`
		);
	}
</script>

<section class="tile panel-box" aria-labelledby="{id}-title">
	<div class="head">
		<h2 id="{id}-title" class="group-title" tabindex="-1" bind:this={heading}>On disk</h2>
		<span class="hint">{plural(c.torrents.length, 'release')}</span>
	</div>
	{#if c.torrents.length}
		<ul class="plain-list releases" bind:this={list}>
			{#each c.torrents as t (t.infohash)}
				{@const main = mainVideo(t)}
				{@const reason = `${id}-why-${t.infohash}`}
				<li data-infohash={t.infohash}>
					<p class="release">{nameOf(t)}</p>
					<p class="hint">{facts(t)}</p>
					{#if downloading(t)}
						<StatusLine tone={t.state === 'error' ? 'warn' : 'busy'} text="Downloading · {percent(t.progress_pct)} · {eta(t)}" />
					{/if}
					<div class="actions">
						{#if c.kind === 'movie' && main}
							<a class="btn primary" href={watchHref(t.infohash, main.index)} aria-label="Play: {nameOf(t)}"
								><Icon name="play" size={16} />Play</a
							>
						{/if}
						{#if t.can_delete}
							<ConfirmDialog
								label="Delete"
								ariaLabel="Delete {nameOf(t)}"
								icon="trash-2"
								ghost
								danger
								busy={g.is(`delete:${t.infohash}`)}
								title="Delete this release?"
								description="{nameOf(t)} leaves the disk for everyone in the house. Watch history is kept."
								action="Delete release"
								onconfirm={() => remove(t)}
							/>
						{:else}
							<button class="btn ghost danger" aria-label="Delete {nameOf(t)}" {...unavailable(reason)} onclick={() => ui.say(NO_DELETE)}>
								<Icon name="trash-2" />Delete
							</button>
							<p class="hint" id={reason}>{NO_DELETE}</p>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="hint">Nothing of this title is on disk any more.</p>
	{/if}
	<a class="link-btn" href="/library?view=torrents">Manage releases</a>
</section>

<style>
	.panel-box {
		align-content: start;
	}
	.head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: var(--s-3);
	}
	.releases {
		display: grid;
		gap: var(--s-3);
	}
	.releases li {
		display: grid;
		gap: var(--s-1);
		padding-bottom: var(--s-3);
		border-bottom: 1px solid var(--line);
	}
	.releases li:last-child {
		border-bottom: 0;
		padding-bottom: 0;
	}
	.release {
		margin: 0;
		font: var(--t-secondary);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.actions :global(.btn) {
		min-height: var(--control-h);
	}
	.link-btn {
		display: inline-flex;
		align-items: center;
		min-height: var(--control-h);
	}
</style>
