<script lang="ts">
	// The season's episodes (or the torrent's other files) beside the player: what is watched,
	// where one stopped, the one playing now, and the way to each (play, download, or grab one
	// that is only discovered).
	import { torrents } from '@iris/api/client';
	import { percent } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import { pending } from '#lib/gesture.svelte.ts';
	import type { SideRow } from './episodes.ts';

	interface Props {
		title: string;
		rows: SideRow[];
		ongrab: (row: SideRow) => void;
		/** The row whose grab is in flight. */
		grabbing: (row: SideRow) => boolean;
	}
	let { title, rows, ongrab, grabbing }: Props = $props();
	const id = $props.id();
</script>

<section class="panel-side" aria-labelledby="{id}-title">
	<h2 id="{id}-title" class="group-title">{title} <span class="count">({rows.length})</span></h2>
	<ul class="rows">
		{#each rows as row (row.key)}
			{@const started = !row.watched && row.watchedPct != null && row.watchedPct > 0}
			<li class:active={row.active} aria-current={row.active ? 'true' : undefined}>
				<div class="what">
					<span class="primary" class:mono={row.mono} title={row.primary}>{row.primary}</span>
					<span class="facts">
						{#if row.secondary}<span>{row.secondary}</span>{/if}
						{#if row.watched}<span class="ok"><Icon name="circle-check" size={14} />Watched</span>{/if}
						{#if started}<span class="ok">{percent(row.watchedPct!)} watched</span>{/if}
						{#if row.active}<span>Now playing</span>{/if}
						{#if row.grab}<span>Not downloaded yet</span>{/if}
					</span>
					{#if started}
						<span class="bar" aria-hidden="true"><span style:width="{row.watchedPct}%"></span></span>
					{/if}
				</div>
				<div class="actions">
					{#if row.grab}
						<button class="btn primary" type="button" {...pending(grabbing(row))} onclick={() => ongrab(row)}>
							<Icon name="download" busy={grabbing(row)} />Grab and play<span class="sr-only">, {row.primary}</span>
						</button>
					{:else if row.active}
						<span class="btn ghost now">Playing</span>
					{:else}
						<a class="btn primary" href="/watch/{row.infohash}/{row.fileIdx}">
							<Icon name="play" />{started ? 'Resume' : 'Play'}<span class="sr-only">, {row.primary}</span>
						</a>
					{/if}
					{#if !row.grab}
						<a
							class="btn icon"
							href={torrents.downloadUrl(row.infohash, row.fileIdx)}
							download={row.primary}
							aria-label="Download {row.primary}"
						>
							<Icon name="download" />
						</a>
					{/if}
				</div>
			</li>
		{/each}
	</ul>
</section>

<style>
	.panel-side {
		display: grid;
		gap: var(--s-3);
		align-content: start;
		padding: var(--s-4);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
		min-width: 0;
	}
	.count {
		color: var(--ink-muted);
		font: var(--t-secondary);
	}
	.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--s-1);
	}
	.rows li {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		gap: var(--s-2);
		align-items: center;
		padding: var(--s-2);
		border-radius: var(--radius-m);
	}
	.rows li.active {
		background: var(--accent-wash);
	}
	.what {
		display: grid;
		gap: var(--s-1);
		min-width: 0;
	}
	.primary {
		font: var(--t-label);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.primary.mono {
		font: var(--t-meta);
		font-family: var(--font-mono);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-1) var(--s-2);
		font: var(--t-meta);
		color: var(--ink-muted);
	}
	.ok {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		color: var(--accent);
	}
	.bar {
		display: block;
		height: var(--s-half);
		border-radius: var(--radius-pill);
		background: var(--line);
		overflow: hidden;
	}
	.bar span {
		display: block;
		height: 100%;
		background: var(--accent);
	}
	.actions {
		display: flex;
		gap: var(--s-1);
		align-items: center;
	}
	.btn.icon {
		padding: 0;
		width: var(--control-h-s);
		justify-content: center;
	}
	.now {
		color: var(--ink-muted);
		cursor: default;
	}
</style>
