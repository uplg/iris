<script lang="ts">
	// The stage's top bar: the way back, the title (the page's h1, in Fraunces: the focus lands
	// here after a navigation, inside the player, so its keys work at once), a line under it, and
	// a quiet facts line. On a phone it keeps to one line: the way back by its icon (its words
	// stay for readers), the facts left out.
	import BackLink from '#lib/components/BackLink.svelte';

	interface Props {
		back: { href: string; label: string; onclick?: (e: MouseEvent) => void };
		title: string;
		sub?: string | null;
		facts?: string | null;
	}
	let { back, title, sub, facts }: Props = $props();
</script>

<div class="topbar">
	<BackLink stage href={back.href} label={back.label} onclick={back.onclick} />
	<div class="heading">
		<h1 tabindex="-1">{title}</h1>
		{#if sub}<p class="sub">{sub}</p>{/if}
	</div>
	{#if facts}<p class="facts">{facts}</p>{/if}
</div>

<style>
	.topbar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-1) var(--s-4);
		color: var(--stage-ink);
	}
	.heading {
		display: grid;
		min-width: 0;
		flex: 1;
	}
	h1 {
		font: var(--t-group);
		font-family: var(--font-display);
		margin: 0;
		overflow-wrap: anywhere;
	}
	.sub,
	.facts {
		margin: 0;
		font: var(--t-secondary);
		color: var(--stage-muted);
	}
	.facts {
		font: var(--t-meta);
	}
	@media (max-width: 599px) {
		.topbar {
			flex-wrap: nowrap;
			gap: var(--s-2);
		}
		.heading {
			display: flex;
			align-items: baseline;
			gap: var(--s-2);
		}
		h1 {
			min-width: 0;
			white-space: nowrap;
			overflow: hidden;
			text-overflow: ellipsis;
		}
		.sub {
			flex: none;
		}
		.facts {
			display: none;
		}
	}
</style>
