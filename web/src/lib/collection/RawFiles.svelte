<script lang="ts">
	// The files themselves, when the episode list has nothing to say: a release without a SCENE
	// identity, a pack the parser never split, a movie copy with no clear main file. The server
	// already orders them (SCENE-aware), so they are listed as they come.
	import type { CollectionDetail } from '@iris/api/client';
	import { formatSize } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import { isVideo, watchHref } from './merge.ts';

	let { collection: c }: { collection: CollectionDetail } = $props();
	const id = $props.id();
	const files = $derived(
		c.torrents.flatMap((t) => t.files.filter((f) => isVideo(f.path)).map((f) => ({ t, f, name: f.path.split('/').pop() ?? f.path })))
	);
</script>

<section class="files" aria-labelledby="{id}-title">
	<h2 id="{id}-title" class="group-title" tabindex="-1">Files</h2>
	{#if files.length}
		<ul class="plain-list">
			{#each files as { t, f, name } (`${t.infohash}:${f.index}`)}
				<li>
					<span class="file">{name}</span>
					<span class="hint">{formatSize(f.size_bytes)}</span>
					<a class="btn" href={watchHref(t.infohash, f.index)} aria-label="Play: {name}"><Icon name="play" size={16} />Play</a>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="hint">No video file is on disk for this title.</p>
	{/if}
</section>

<style>
	.files {
		display: grid;
		gap: var(--s-3);
		max-width: var(--measure-wide);
	}
	li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2) var(--s-3);
		padding: var(--s-2) 0;
		border-bottom: 1px solid var(--line);
	}
	.file {
		flex: 1 1 14rem;
		min-width: 0;
		font: var(--t-secondary);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
