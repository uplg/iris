<script lang="ts">
	// Every page's head, said once: the window title (« Salon · Maison »), the way back for a
	// device page, the one h1 that takes the focus after a navigation (+layout.svelte), a line
	// under it, and its actions on the right (docs/ux.md « Layout »).
	import type { Snippet } from 'svelte';
	import { pageTitle } from '#lib/title.ts';
	import BackLink from './BackLink.svelte';

	interface Props {
		title: string;
		/** A device page: a way back to the dashboard above the title. */
		back?: boolean;
		/** The line under the title. */
		sub?: Snippet;
		end?: Snippet;
		/** The title is known without being read (the dashboard: « Accueil »): kept for screen
		 * readers and for the focus after a navigation, not drawn. */
		hidden?: boolean;
	}
	let { title, back = false, sub, end, hidden = false }: Props = $props();
</script>

<svelte:head><title>{pageTitle(title)}</title></svelte:head>

{#if back}
	<BackLink href="/" label="Home" />
{/if}
<div class="page-head" class:quiet={hidden}>
	<h1 tabindex="-1" class:sr-only={hidden}>{title}</h1>
	{#if end}<div class="end">{@render end()}</div>{/if}
	{#if sub}<p class="sub">{@render sub()}</p>{/if}
</div>

<style>
	/* a title not drawn: only the breathing space under the header stays */
	.page-head.quiet {
		padding-block: var(--s-4) 0;
	}
	/* the sub line under the title, the actions beside it */
	.sub {
		flex-basis: 100%;
		order: 3;
	}
</style>
