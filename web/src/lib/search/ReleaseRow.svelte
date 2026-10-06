<script lang="ts">
	// One release in the List view: its title (the link to its details), its name as the tracker
	// wrote it, what it carries, its facts, and the one action: play it from disk when it is
	// there, else grab and play (not operable, and saying why, when the swarm is empty).
	import { goto } from '$app/navigation';
	import type { SearchResult } from '@iris/api/client';
	import { pending, unavailable } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Poster from '#lib/components/Poster.svelte';
	import { Grab, type GrabTarget } from '#lib/release/grab.svelte.ts';
	import GrabNotice from '#lib/release/GrabNotice.svelte';
	import { DEAD, factsLine, isDead, ownedFile, releaseChips, releaseHref, titleLine, titleOf } from './release.ts';
	import { watchHref } from '#lib/paths.ts';

	let { r }: { r: SearchResult } = $props();
	const id = $props.id();
	const owned = $derived(ownedFile(r));
	const dead = $derived(isDead(r.seeders));
	const grab = new Grab((href) => goto(href));
	const target = $derived<GrabTarget>({ provider: r.provider_id, id: r.external_id, tmdbId: r.title_match?.tmdb_id ?? r.tmdb_id });
</script>

<li class="row">
	<div class="mini"><Poster src={r.poster_url} title={titleOf(r)} /></div>
	<div class="body">
		<a class="name" href={releaseHref(r)}>{titleLine(r)}</a>
		<span class="release">{r.title}</span>
		<span class="chips">
			{#if owned}<span class="chip accent"><Icon name="circle-check" size={14} />In your library</span>{/if}
			{#if r.freeleech}<span class="chip accent">Freeleech</span>{/if}
			{#each releaseChips(r) as c (c)}<span class="chip">{c}</span>{/each}
		</span>
		<span class="facts">{factsLine(r)}</span>
		{#if dead && !owned}<span class="dead" id="{id}-dead"><Icon name="triangle-alert" size={14} />{DEAD}</span>{/if}
		<GrabNotice {grab} {target} />
	</div>
	<div class="end">
		{#if owned}
			<a class="btn primary act" href={watchHref(owned.infohash, owned.idx)}><Icon name="play" />Play from disk</a>
		{:else}
			<button
				class="btn primary act"
				{...pending(grab.busy)}
				{...unavailable(dead && `${id}-dead`)}
				onclick={() => !dead && grab.run(target)}
			>
				<Icon name="download" busy={grab.busy} />Grab and play
			</button>
		{/if}
	</div>
</li>

<style>
	.row {
		display: grid;
		grid-template-columns: var(--poster-mini) minmax(0, 1fr);
		gap: var(--s-2) var(--s-4);
		padding: var(--s-3) 0;
		border-bottom: 1px solid var(--line);
	}
	.mini {
		width: var(--poster-mini);
	}
	/* a mini poster without artwork: the glyph alone, its title is the row's link */
	.mini :global(.fallback) {
		align-items: center;
		justify-content: center;
	}
	.mini :global(.fallback span) {
		display: none;
	}
	.body {
		display: grid;
		gap: var(--s-1);
		align-content: start;
		min-width: 0;
	}
	.end {
		grid-column: 2;
	}
	@media (min-width: 600px) {
		.row {
			grid-template-columns: var(--poster-mini) minmax(0, 1fr) auto;
		}
		.end {
			grid-column: 3;
		}
	}
	.name {
		font: var(--t-body);
		font-weight: 600;
		color: var(--ink);
		text-decoration: none;
	}
	.name:hover {
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.release {
		font: var(--t-secondary);
		font-family: var(--font-mono);
		overflow-wrap: anywhere;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-1);
	}
	.facts,
	.dead {
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
	.dead {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		color: var(--warn-text);
	}
	.act {
		min-height: var(--control-h);
		white-space: nowrap;
	}
</style>
