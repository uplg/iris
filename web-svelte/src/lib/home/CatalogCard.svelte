<script lang="ts">
	// A suggested title (For You shelves, a mood's results): it names a title, not a release,
	// so it leads to a search for it, where releases are ranked and previewed before anything
	// downloads. "Not interested" hides it from every suggestion surface, on the server's word.
	import { me, type CatalogCard } from '@iris/api/client';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus, sectionHeading } from '#lib/focus.ts';
	import { kindLabel } from './data.ts';
	import { KEYS } from '#lib/queries.ts';
	import { plural } from '@iris/api/format';

	let { card }: { card: CatalogCard } = $props();
	const g = new Gesture();
	let button = $state<HTMLButtonElement>();

	const meta = $derived(
		[kindLabel(card.kind, card.is_anime), card.year ? String(card.year) : null, card.seeders ? plural(card.seeders, 'seeder') : null]
			.filter(Boolean)
			.join(' · ')
	);
	const status = $derived(
		card.already_in_library
			? { tone: 'ok' as const, text: 'In your library' }
			: card.reason
				? { tone: 'info' as const, text: card.reason }
				: undefined
	);

	function dismiss() {
		const heading = sectionHeading(button);
		return g.run(
			() => me.dismissForYou(card.catalog_id),
			async () => {
				await Promise.all([KEYS.forYou, KEYS.forYouPage, KEYS.moodResults].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
				ui.say(`${card.title} hidden from your suggestions`);
				await refocus(heading, 'main h1');
			}
		);
	}
</script>

<PosterCard href="/search?q={encodeURIComponent(card.title)}" title={card.title} art={card.poster_url} {meta} {status}>
	{#snippet actions()}
		<button class="icon-btn hide" aria-label="Not interested in {card.title}" {...pending(g.is())} onclick={dismiss} bind:this={button}>
			<Icon name="x" busy={g.is()} />
		</button>
	{/snippet}
</PosterCard>

<style>
	.hide {
		flex: none;
		width: var(--control-h);
		height: var(--control-h);
	}
</style>
