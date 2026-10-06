<script lang="ts">
	// A release's video files: each with its size, how much of it you watched, and a way to play
	// (or resume) it and to save it.
	import { torrents, type ContinueWatchingItem, type FileEntry } from '@iris/api/client';
	import { formatSize, percent } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import { watchState } from './model.ts';

	let { infohash, files, watched }: { infohash: string; files: FileEntry[]; watched?: Map<number, ContinueWatchingItem> } = $props();
</script>

<ul class="plain-list files">
	{#each files as f (f.index)}
		{@const name = f.path.split('/').pop() ?? f.path}
		{@const w = watchState(watched?.get(f.index))}
		{@const resume = w.pct !== null && w.pct > 0 && !w.done}
		<li class="file">
			<div class="text">
				<span class="path">{f.path}</span>
				<span class="meta">
					{formatSize(f.size_bytes)}{#if w.done}{' · '}<Icon
							name="circle-check"
							size={14}
						/>Watched{:else if w.pct !== null}{` · ${percent(w.pct)} watched`}{/if}
				</span>
			</div>
			<div class="end">
				<a class="btn" href="/watch/{infohash}/{f.index}" aria-label="{resume ? 'Resume' : 'Play'} {name}"
					><Icon name="play" />{resume ? 'Resume' : 'Play'}</a
				>
				<a class="btn ghost" href={torrents.downloadUrl(infohash, f.index)} download={name} aria-label="Save {name}"
					><Icon name="download" />Save</a
				>
			</div>
		</li>
	{/each}
</ul>

<style>
	.files {
		display: grid;
	}
	.file {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-2) var(--s-3);
		padding: var(--s-2) 0;
		border-top: 1px solid var(--line);
	}
	.text {
		display: grid;
		gap: var(--s-half);
		min-width: 0;
		flex: 1 1 14rem;
	}
	.path {
		font: var(--t-meta);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.meta {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		font: var(--t-meta);
		color: var(--ink-muted);
		font-variant-numeric: tabular-nums;
	}
	.end {
		display: flex;
		gap: var(--s-2);
	}
	.end .btn {
		min-height: var(--control-h);
	}
</style>
