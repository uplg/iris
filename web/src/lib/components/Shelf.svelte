<script lang="ts">
	// A row of cards under a title (a section, its h2, one fact). The row scrolls sideways
	// with the keyboard and touch like any list; on a pointer, two buttons page it. Nothing
	// moves on its own (no autoplay, no rotation).
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	interface Props {
		title: string;
		/** One fact about the row: "4 titles", "3 on disk". */
		fact?: string;
		/** "See all": where the whole list lives. */
		href?: string;
		/** The cards, as `<li>`s. */
		children: Snippet;
	}
	let { title, fact, href, children }: Props = $props();
	const id = $props.id();
	let rail = $state<HTMLUListElement>();
	let atStart = $state(true);
	let atEnd = $state(false);

	function measure() {
		if (!rail) return;
		atStart = rail.scrollLeft <= 1;
		atEnd = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
	}

	$effect(() => {
		if (!rail) return;
		measure();
		const ro = new ResizeObserver(measure);
		ro.observe(rail);
		rail.addEventListener('scroll', measure, { passive: true });
		return () => {
			ro.disconnect();
			rail?.removeEventListener('scroll', measure);
		};
	});

	const page = (dir: 1 | -1) => rail?.scrollBy({ left: dir * rail.clientWidth * 0.9, behavior: 'smooth' });
</script>

<section class="shelf" aria-labelledby="{id}-title">
	<div class="head">
		<h2 id="{id}-title">{title}</h2>
		{#if fact}<span class="meta">{fact}</span>{/if}
		<div class="grow"></div>
		{#if href}<a class="link-btn" {href}>See all</a>{/if}
		<button class="icon-btn pager" aria-label="Scroll {title} back" aria-disabled={atStart} onclick={() => !atStart && page(-1)}>
			<Icon name="chevron-left" />
		</button>
		<button class="icon-btn pager" aria-label="Scroll {title} forward" aria-disabled={atEnd} onclick={() => !atEnd && page(1)}>
			<Icon name="chevron-right" />
		</button>
	</div>
	<ul class="rail" bind:this={rail}>
		{@render children()}
	</ul>
</section>

<style>
	.shelf {
		display: grid;
		gap: var(--s-3);
		min-width: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: var(--s-3);
		min-height: var(--control-h);
	}
	h2 {
		font: var(--t-group);
		font-family: var(--font-display);
		margin: 0;
	}
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
	.grow {
		flex: 1;
	}
	.rail {
		display: flex;
		gap: var(--s-4);
		overflow-x: auto;
		list-style: none;
		margin: 0;
		padding: 0 0 var(--s-2);
		scroll-snap-type: x proximity;
		scrollbar-width: thin;
	}
	.rail :global(> li) {
		scroll-snap-align: start;
	}
	@media (pointer: coarse) {
		.pager {
			display: none;
		}
	}
</style>
