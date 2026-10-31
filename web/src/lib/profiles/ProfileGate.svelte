<script lang="ts">
	// An account with several profiles asks who is watching, once per tab, before anything is read
	// for the wrong person: over the whole page, each person by their mascot. An account with one,
	// or a failed read, goes straight in.
	import { createQuery } from '@tanstack/svelte-query';
	import type { Snippet } from 'svelte';
	import { read } from '#lib/queries.ts';
	import { queryClient } from '#lib/query.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Mascot from '#lib/components/Mascot.svelte';
	import { chosen } from './chosen.svelte.ts';
	import { enter } from './enter.ts';

	const MASCOT = 112;

	let { children }: { children: Snippet } = $props();

	const list = createQuery(
		() => ({ ...read.profiles(), enabled: !chosen.done }),
		() => queryClient
	);
	const g = new Gesture();
	const asking = $derived(!chosen.done && (list.data?.length ?? 0) > 1);
	const waiting = $derived(!chosen.done && list.isPending);
</script>

{#if asking}
	<section class="who" aria-labelledby="who-title">
		<h1 id="who-title" tabindex="-1">Who’s watching?</h1>
		<ul>
			{#each list.data ?? [] as p (p.id)}
				<li>
					<button class="person" {...pending(g.is(p.id))} onclick={() => g.run(() => enter(p), undefined, p.id)}>
						<Mascot name={p.name} size={MASCOT} />
						<span>{p.name}</span>
					</button>
				</li>
			{/each}
		</ul>
		{#if g.error}<p class="form-error" role="alert">{g.error}</p>{/if}
	</section>
{:else if !waiting}
	{@render children()}
{/if}

<style>
	.who {
		position: fixed;
		inset: 0;
		z-index: 30;
		display: grid;
		align-content: center;
		justify-items: center;
		gap: var(--s-6);
		padding: var(--s-6) var(--page-margin);
		overflow-y: auto;
		background: var(--ground);
	}
	h1 {
		margin: 0;
		font: var(--t-page);
		text-align: center;
	}
	ul {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: var(--s-5);
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.person {
		display: grid;
		justify-items: center;
		gap: var(--s-3);
		padding: var(--s-3);
		border: 0;
		border-radius: var(--radius);
		background: none;
		color: var(--ink-muted);
		font: var(--t-label);
		cursor: pointer;
	}
	.person:hover {
		color: var(--ink);
	}
	.person:focus-visible {
		color: var(--ink);
		outline: var(--focus-ring);
		outline-offset: var(--focus-offset);
	}
	@media (prefers-reduced-motion: no-preference) {
		.person :global(.mascot) {
			transition: transform 160ms ease-out;
		}
		.person:hover :global(.mascot),
		.person:focus-visible :global(.mascot) {
			transform: translateY(-4px) rotate(-3deg);
		}
	}
</style>
