<script lang="ts">
	// What friends watch and add, a line each: who is playing right now first, then what is
	// recent. Nothing at all without friends or news. Read again by the query, never a timer of ours.
	import { createQuery } from '@tanstack/svelte-query';
	import { ago } from '@iris/api/format';
	import Mascot from '#lib/components/Mascot.svelte';
	import { read } from '#lib/queries.ts';
	import { friendRows } from './data.ts';

	const VISIT_MS = 30_000;

	const activity = createQuery(() => ({ ...read.friendActivity(), refetchInterval: VISIT_MS }));
	const rows = $derived(activity.data ? friendRows(activity.data) : []);
</script>

{#if rows.length}
	<section class="friends" aria-labelledby="friends-now-title">
		<h2 id="friends-now-title" class="eyebrow">Friends</h2>
		<ul class="plain-list">
			{#each rows as row (row.friend_id)}
				<li>
					<Mascot name={row.name} />
					<span class="line">
						<span>
							<strong>{row.name}</strong>{#each row.items as item, i (item.key)}{i ? ', ' : ' '}{item.verb
									? `${item.verb} `
									: ''}{#if item.collection_id}<a href="/collection/{item.collection_id}">{item.title}</a>{:else}{item.title}{/if}{/each}
						</span>
						{#if row.at}<span class="when">{ago(row.at, 'short')}</span>{/if}
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
	strong {
		font-weight: 600;
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
