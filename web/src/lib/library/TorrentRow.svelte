<script lang="ts">
	// One release: its title (to its title's page), the release name, how far it is, its state in
	// words, who added it, and what can be done: play it, open its files, delete it. Delete is
	// for an admin or whoever added it; anyone else sees it, not operable, and why.
	import { tmdbImage, torrents, type CollectionListItem, type ContinueWatchingItem, type TorrentView } from '@iris/api/client';
	import { formatSize, isVideo, percent, when } from '@iris/api/format';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, unavailable } from '#lib/gesture.svelte.ts';
	import Poster from '#lib/components/Poster.svelte';
	import Progress from '#lib/components/Progress.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
	import Disclosure from '#lib/components/Disclosure.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { deleteDescription, ratioOf, releaseName, releaseStatus, watchState } from './model.ts';
	import { refreshLibrary } from '#lib/queries.ts';
	import FileList from './FileList.svelte';

	interface Props {
		t: TorrentView;
		title: string;
		collection?: CollectionListItem;
		/** The caller's watch state, per file index. */
		watched?: Map<number, ContinueWatchingItem>;
		/** Deleted and read again: the row is gone, the focus is the caller's to place. */
		ondeleted: () => unknown;
	}
	let { t, title, collection, watched, ondeleted }: Props = $props();
	const id = $props.id();
	const g = new Gesture();

	const videos = $derived(t.files.filter((f) => isVideo(f.path)));
	const status = $derived(releaseStatus(t));
	const pct = $derived(Math.min(100, Math.max(0, t.progress_pct)));
	const showBar = $derived(!t.finished && pct < 100);
	const ratio = $derived(ratioOf(t.uploaded_bytes_total, t.downloaded_bytes_total));
	const facts = $derived(
		[
			`Added by ${t.added_by_name}`,
			when(Date.parse(t.added_at)),
			t.source_provider && `from ${t.source_provider}`,
			`${formatSize(t.uploaded_bytes_total)} sent`,
			ratio !== null && `ratio ${ratio.toFixed(2)}`
		]
			.filter(Boolean)
			.join(' · ')
	);
	const single = $derived(videos.length === 1 ? videos[0] : undefined);
	const singleWatch = $derived(single ? watchState(watched?.get(single.index)) : null);

	function remove() {
		return g.run(
			() => torrents.remove(t.infohash),
			async () => {
				await refreshLibrary();
				ui.toast(`Deleted ${title}`);
				await ondeleted();
			},
			'delete'
		);
	}
</script>

<li class="row whole-card">
	<div class="thumb"><Poster src={tmdbImage(collection?.poster_path, 'w154')} {title} /></div>
	<div class="body">
		<h3 class="name">
			{#if t.collection_id}<a class="card-link" href="/collection/{t.collection_id}">{title}</a>{:else}{title}{/if}
		</h3>
		<p class="release" title={t.name ?? undefined}>{t.name ? releaseName(t.name) : t.infohash}</p>
		{#if showBar}
			<Progress label="{formatSize(t.progress_bytes)} of {formatSize(t.total_size_bytes)}" value={pct} max={100} valueText={percent(pct)} />
		{/if}
		<StatusLine tone={status.tone} text={status.text} />
		<p class="facts">{facts}</p>
		{#if videos.length > 1}
			<Disclosure label="{videos.length} video files">
				<FileList infohash={t.infohash} files={videos} {watched} />
			</Disclosure>
		{/if}
	</div>
	<div class="actions">
		{#if single}
			<a
				class="btn"
				href="/watch/{t.infohash}/{single.index}"
				aria-label="{singleWatch?.pct && !singleWatch.done ? 'Resume' : 'Play'} {title}"
			>
				<Icon name="play" />{singleWatch?.pct && !singleWatch.done ? 'Resume' : 'Play'}
			</a>
		{/if}
		{#if t.can_delete === true}
			<ConfirmDialog
				label="Delete"
				ariaLabel="Delete {title}"
				icon="trash-2"
				ghost
				danger
				busy={g.is('delete')}
				title="Delete {title}?"
				description={deleteDescription(t)}
				action="Delete"
				onconfirm={remove}
			/>
		{:else}
			<button class="btn ghost danger" aria-label="Delete {title}" {...unavailable(`${id}-why`)}><Icon name="trash-2" />Delete</button>
			<span id="{id}-why" class="why">Only an admin or {t.added_by_name} can delete this</span>
		{/if}
	</div>
</li>

<style>
	.row {
		display: grid;
		grid-template-columns: var(--poster-mini) minmax(0, 1fr);
		gap: var(--s-2) var(--s-4);
		padding: var(--s-4) 0;
		border-bottom: 1px solid var(--line);
	}
	.thumb {
		grid-row: span 2;
	}
	/* too narrow for the title in the artwork's place: the glyph alone, the row names it */
	.thumb :global(.fallback) {
		justify-content: center;
		align-items: center;
	}
	.thumb :global(.fallback span) {
		display: none;
	}
	.body {
		display: grid;
		gap: var(--s-2);
		min-width: 0;
	}
	.name {
		font: var(--t-body);
		font-weight: 600;
		margin: 0;
		overflow-wrap: anywhere;
	}
	.name a {
		color: var(--ink);
		text-decoration: none;
	}
	.name a:hover {
		text-decoration: underline;
		text-underline-offset: var(--s-half);
	}
	.release {
		margin: 0;
		font: var(--t-meta);
		font-family: var(--font-mono);
		color: var(--ink-muted);
		overflow-wrap: anywhere;
	}
	.facts {
		margin: 0;
		font: var(--t-meta);
		color: var(--ink-muted);
		font-variant-numeric: tabular-nums;
	}
	.actions {
		grid-column: 2;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
	}
	.actions :global(.btn) {
		min-height: var(--control-h);
	}
	.why {
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	@media (min-width: 840px) {
		.row {
			grid-template-columns: var(--poster-mini) minmax(0, 1fr) auto;
		}
		.actions {
			grid-column: 3;
			grid-row: 1;
			align-self: start;
			justify-content: flex-end;
			max-width: var(--pane);
		}
	}
</style>
