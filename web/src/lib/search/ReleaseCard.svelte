<script lang="ts">
	// One release in the Grid view: its poster with the tracker on it, the title it belongs to
	// (the one link: its details, or straight to the player when it is already on disk), what
	// part and which audio, the picture and the swarm, then its state in words.
	import type { SearchResult } from '@iris/api/client';
	import { languageLabel } from '@iris/api/format';
	import Poster from '#lib/components/Poster.svelte';
	import StatusLine from '#lib/components/StatusLine.svelte';
	import { DEAD, isDead, ownedFile, partWords, releaseHref, resolution, seedersWords, titleOf } from './release.ts';
	import { watchHref } from '#lib/paths.ts';

	let { r }: { r: SearchResult } = $props();
	const owned = $derived(ownedFile(r));
	const title = $derived(titleOf(r));
	const what = $derived(
		[partWords(r.parsed_season, r.parsed_episode, r.title), languageLabel(r.language_tag, 'short')].filter(Boolean).join(' · ')
	);
	const how = $derived([resolution(r.title), seedersWords(r.seeders)].filter(Boolean).join(' · '));
</script>

<li class="card">
	<div class="art">
		<Poster src={r.poster_url} {title} />
		<span class="chip tracker">{r.provider_id}</span>
	</div>
	<div class="lines">
		<a class="name" href={owned ? watchHref(owned.infohash, owned.idx) : releaseHref(r)}>{title}</a>
		<span class="release" title={r.title}>{r.title}</span>
		{#if what}<span class="meta">{what}</span>{/if}
		{#if how}<span class="meta">{how}</span>{/if}
		{#if owned}
			<StatusLine tone="ok" text="In your library · plays from disk" />
		{:else if isDead(r.seeders)}
			<StatusLine tone="warn" text={DEAD} />
		{/if}
	</div>
</li>

<style>
	.card {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: var(--s-2);
		align-content: start;
		min-width: 0;
	}
	.art {
		position: relative;
	}
	.tracker {
		position: absolute;
		top: var(--s-2);
		right: var(--s-2);
		background: var(--surface);
		color: var(--ink);
		border: 1px solid var(--line);
	}
	.lines {
		display: grid;
		gap: var(--s-half);
		min-width: 0;
	}
	.name {
		font: var(--t-body);
		font-weight: 600;
		color: var(--ink);
		text-decoration: none;
		overflow-wrap: anywhere;
	}
	.name:hover {
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.release {
		font: var(--t-tiny);
		font-family: var(--font-mono);
		color: var(--ink-muted);
		overflow-wrap: anywhere;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.meta {
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
</style>
