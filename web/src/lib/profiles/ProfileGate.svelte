<script lang="ts">
	// An account with several profiles asks who is watching, once per tab, before anything is read
	// for the wrong person. An account with one, or a failed read, goes straight in.
	import { createQuery } from '@tanstack/svelte-query';
	import type { Snippet } from 'svelte';
	import { read } from '#lib/queries.ts';
	import { queryClient } from '#lib/query.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { chosen } from './chosen.svelte.ts';
	import { enter } from './enter.ts';

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
					<button class="btn big" {...pending(g.is(p.id))} onclick={() => g.run(() => enter(p), undefined, p.id)}>{p.name}</button>
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
		display: grid;
		justify-items: center;
		gap: var(--s-5);
		padding: var(--s-7) var(--page-margin);
	}
	h1 {
		margin: 0;
	}
	ul {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: var(--s-3);
		margin: 0;
		padding: 0;
		list-style: none;
	}
</style>
