<script lang="ts">
	// A suggested title (For You shelves, a mood's results): it names a title, not a release,
	// so it leads to a search for it, where releases are ranked and previewed before anything
	// downloads. Its synopsis opens on the side, leaving the page where it is (no synopsis, no
	// button). "Not interested" hides it from every suggestion surface, on the server's word.
	import { me, type CatalogCard } from '@iris/api/client';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus, sectionHeading } from '#lib/focus.ts';
	import { KEYS } from '#lib/queries.ts';
	import { kindLabel } from '@iris/api/format';
	import { seedersWords } from '#lib/search/release.ts';

	let { card }: { card: CatalogCard } = $props();
	const g = new Gesture();
	let button = $state<HTMLButtonElement>();
	let reading = $state(false);
	const searchHref = $derived(`/search?q=${encodeURIComponent(card.title)}`);
	const overview = $derived(card.overview?.trim());

	const meta = $derived(
		[kindLabel(card.kind, card.is_anime), card.year ? String(card.year) : null, card.seeders ? seedersWords(card.seeders) : null]
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

<PosterCard href={searchHref} title={card.title} art={card.poster_url} {meta} {status}>
	{#snippet artAction()}
		{#if overview}
			<button class="icon-btn synopsis" aria-label="Read the synopsis of {card.title}" onclick={() => (reading = true)}>
				<Icon name="info" />
			</button>
		{/if}
	{/snippet}
	{#snippet actions()}
		<button class="icon-btn hide" aria-label="Not interested in {card.title}" {...pending(g.is())} onclick={dismiss} bind:this={button}>
			<Icon name="x" busy={g.is()} />
		</button>
	{/snippet}
</PosterCard>

<Sheet open={reading} onclose={() => (reading = false)} title={card.title} description={meta}>
	<p class="overview">{overview}</p>
	<div class="actions">
		<a class="btn primary" href={searchHref} onclick={() => (reading = false)}>Find releases</a>
		<button class="btn" {...pending(g.is())} onclick={dismiss}>
			{#if g.is()}<Icon name="loader-circle" busy />{/if}Not interested
		</button>
	</div>
</Sheet>

<style>
	.hide {
		flex: none;
		width: var(--control-h);
		height: var(--control-h);
	}
	.synopsis {
		width: var(--control-h);
		height: var(--control-h);
		border-radius: var(--radius-pill);
		background: var(--surface);
		color: var(--ink);
		border: 1px solid var(--line);
	}
	.synopsis:hover {
		background: var(--surface);
	}
	.actions > .btn {
		min-height: var(--control-h);
	}
	.overview {
		margin: 0 0 var(--s-5);
		font: var(--t-body);
		max-width: var(--measure);
	}
</style>
