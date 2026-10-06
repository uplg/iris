<script lang="ts">
	// What the library already holds for this search, before any tracker's release: its poster,
	// what is on disk, and the way in (the episode asked for plays at once; else its collection).
	import { tmdbImage, type LibraryMatch } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';
	import Poster from '#lib/components/Poster.svelte';
	import { matchTarget } from './release.ts';
	import { kindWord } from '@iris/api/format';

	let { matches }: { matches: readonly LibraryMatch[] } = $props();
	const id = $props.id();
</script>

<section class="lib" aria-labelledby="{id}-title">
	<h2 id="{id}-title" class="group-title">In your library</h2>
	<ul class="plain-list">
		{#each matches as m (m.collection_id)}
			{@const t = matchTarget(m)}
			<li class="match">
				<div class="mini"><Poster src={tmdbImage(m.poster_path, 'w342')} title={m.display_title} /></div>
				<div class="body">
					<a class="name" href="/collection/{m.collection_id}">{m.display_title}</a>
					<span class="meta">{[kindWord(m.kind), t.facts].filter(Boolean).join(' · ')}</span>
				</div>
				<a class="btn primary act" href={t.href} aria-label="{t.action}: {m.display_title}">
					<Icon name={t.action === 'Open' ? 'arrow-right' : 'play'} />{t.action}
				</a>
			</li>
		{/each}
	</ul>
</section>

<style>
	.lib {
		display: grid;
		gap: var(--s-3);
	}
	.match {
		display: grid;
		grid-template-columns: var(--poster-mini) minmax(0, 1fr);
		gap: var(--s-2) var(--s-4);
		align-items: center;
		padding: var(--s-3) var(--s-3);
		border: 1px solid var(--line);
		border-radius: var(--radius-l);
		background: var(--surface);
	}
	.match + .match {
		margin-top: var(--s-2);
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
		min-width: 0;
	}
	.act {
		grid-column: 2;
		justify-self: start;
		min-height: var(--control-h);
	}
	@media (min-width: 600px) {
		.match {
			grid-template-columns: var(--poster-mini) minmax(0, 1fr) auto;
		}
		.act {
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
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
</style>
