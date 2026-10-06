<script lang="ts">
	// The curated moods for a kind, the account's genres first: each a tile with the backdrop
	// of its top title, its name, and that title in words. A tile is a link (the mood is in the
	// address: shareable, and Back returns to the board).
	import { createQuery } from '@tanstack/svelte-query';
	import { me, type MediaKind } from '@iris/api/client';
	import Loaded from '#lib/components/Loaded.svelte';
	import { loadable } from '#lib/query.ts';
	import { KEYS } from '#lib/home/data.ts';

	let { kind }: { kind: MediaKind } = $props();
	const board = createQuery(() => ({ queryKey: KEYS.moodBoard(kind), queryFn: () => me.moodBoard(kind) }));
	const moods = $derived(board.data?.moods ?? []);
</script>

<Loaded
	value={loadable(board)}
	empty={moods.length === 0}
	emptyText="No mood has anything to get right now."
	emptyHint="Moods fill in as your trackers carry new releases."
	skeletons={4}
>
	<ul class="tiles moods">
		{#each moods as mood (mood.id)}
			<li>
				<a class="tile mood" href="/discover?mood={encodeURIComponent(mood.id)}&kind={kind}">
					<span class="art" aria-hidden="true">
						{#if mood.backdrop_url}<img src={mood.backdrop_url} alt="" width="500" height="281" loading="lazy" decoding="async" />{/if}
					</span>
					<span class="name">{mood.label}</span>
					{#if mood.featured_title}<span class="now">Now: {mood.featured_title}</span>{/if}
				</a>
			</li>
		{/each}
	</ul>
</Loaded>

<style>
	.moods {
		grid-template-columns: repeat(auto-fill, minmax(min(13rem, 100%), 1fr));
		gap: var(--s-4);
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.mood {
		gap: var(--s-2);
		padding: var(--s-2) var(--s-2) var(--s-3);
		color: var(--ink);
		text-decoration: none;
	}
	.mood:hover {
		border-color: var(--accent-soft);
	}
	.art {
		display: block;
		aspect-ratio: 16 / 9;
		overflow: hidden;
		border-radius: var(--radius-m);
		background: var(--accent-wash);
	}
	img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.name {
		font: var(--t-group);
		padding-inline: var(--s-1);
	}
	.now {
		font: var(--t-meta);
		color: var(--ink-muted);
		padding-inline: var(--s-1);
		overflow-wrap: anywhere;
	}
</style>
