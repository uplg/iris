<script lang="ts">
	// What friends watch and add: who is playing right now first, then what is recent. Nothing at
	// all without friends or news. Read again by the query, never a timer of ours.
	import { createQuery } from '@tanstack/svelte-query';
	import { ago } from '@iris/api/format';
	import Mascot from '#lib/components/Mascot.svelte';
	import { read } from '#lib/queries.ts';
	import { friendLine } from './data.ts';

	const VISIT_MS = 30_000;
	const SHOWN = 6;

	const activity = createQuery(() => ({ ...read.friendActivity(), refetchInterval: VISIT_MS }));
	const lines = $derived(activity.data ? [...activity.data.live, ...activity.data.recent].slice(0, SHOWN) : []);
</script>

{#if lines.length}
	<section class="friends" aria-labelledby="friends-now-title">
		<h2 id="friends-now-title" class="eyebrow">Friends</h2>
		<ul class="plain-list">
			{#each lines as e (`${e.action}:${e.friend_id}:${e.collection_id ?? e.title}`)}
				<li>
					<Mascot name={e.friend_name} />
					<span class="line">
						{#if e.collection_id}<a href="/collection/{e.collection_id}">{friendLine(e)}</a>{:else}<span>{friendLine(e)}</span>{/if}
						{#if e.action !== 'watching'}<span class="when">{ago(e.at, 'short')}</span>{/if}
					</span>
				</li>
			{/each}
		</ul>
	</section>
{/if}

<style>
	.friends {
		display: grid;
		gap: var(--s-2);
	}
	li {
		display: flex;
		align-items: center;
		gap: var(--s-3);
		min-height: var(--control-h);
	}
	.line {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0 var(--s-3);
		min-width: 0;
	}
	a {
		color: var(--ink);
		text-decoration: none;
	}
	a:hover {
		color: var(--accent);
		text-decoration: underline;
	}
	.when {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
</style>
