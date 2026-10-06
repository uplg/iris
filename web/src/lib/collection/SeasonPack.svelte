<script lang="ts">
	// A whole season in one release: grab it to play its first episode now, or only prepare it
	// (it downloads; the episodes show as on disk once it is in). The server resolves episode 1
	// inside the pack.
	import { goto } from '$app/navigation';
	import { library, type SeasonPackEntry } from '@iris/api/client';
	import { formatSize } from '@iris/api/format';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus, sectionHeading } from '#lib/focus.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { refetchCollection } from './actions.ts';
	import { audioChip, seasonName, watchHref } from './merge.ts';

	let { collectionId, pack }: { collectionId: string; pack: SeasonPackEntry } = $props();
	const g = new Gesture();
	let box = $state<HTMLElement>();
	let only = $state<HTMLElement>();
	const name = $derived(seasonName(pack.season));
	const facts = $derived(
		[
			pack.language ? audioChip(pack.language) : null,
			pack.quality,
			typeof pack.seeders === 'number' ? `${pack.seeders} seeders` : null,
			typeof pack.size_bytes === 'number' ? formatSize(pack.size_bytes) : null,
			`via ${pack.indexer_provider}`
		]
			.filter(Boolean)
			.join(' · ')
	);
	const grab = () => library.grabCollectionEpisode(collectionId, pack.season, 1, pack.language ?? null);
</script>

<div class="callout pack" bind:this={box}>
	<p><strong>{name}: the full season is available in one release</strong></p>
	<p class="hint">{facts}</p>
	<div class="actions">
		<button
			class="btn primary"
			aria-label="Grab and play: {name.toLowerCase()}, from its first episode"
			{...pending(g.is('play'))}
			onclick={() =>
				g.run(
					grab,
					async (res) => {
						await refetchCollection(collectionId);
						await goto(watchHref(res.infohash, res.file_idx));
					},
					'play'
				)}
		>
			<Icon name="play" size={16} busy={g.is('play')} />Grab and play
		</button>
		<button
			class="btn"
			bind:this={only}
			aria-label="Download only: {name.toLowerCase()}, without playing"
			{...pending(g.is('prepare'))}
			onclick={() => {
				// the banner leaves once the pack is in: the focus goes to the list's title
				const heading = sectionHeading(box);
				return g.run(
					grab,
					async () => {
						await refetchCollection(collectionId);
						ui.toast(`${name} is downloading.`);
						await refocus(only, heading);
					},
					'prepare'
				);
			}}
		>
			<Icon name="download" size={16} busy={g.is('prepare')} />Download only
		</button>
	</div>
</div>

<style>
	.pack .btn {
		min-height: var(--control-h);
	}
</style>
