<script lang="ts">
	// The last searches the server kept for this account: one press searches again, a cross
	// forgets one, « Clear » forgets them all. Shown once the server has answered, never guessed.
	import { createQuery } from '@tanstack/svelte-query';
	import { me } from '@iris/api/client';
	import { queryClient } from '#lib/query.ts';
	import { refocus } from '#lib/focus.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';

	let { onsearch, back }: { onsearch: (q: string) => void; back: () => HTMLElement | null | undefined } = $props();
	const id = $props.id();
	const SHOWN = 5;
	const g = new Gesture();

	const recent = createQuery(
		() => ({ queryKey: ['recent-searches'], queryFn: () => me.recentSearches(), staleTime: 60_000 }),
		() => queryClient
	);
	const shown = $derived((recent.data ?? []).slice(0, SHOWN));

	function forget(query?: string) {
		const at = query === undefined ? -1 : shown.findIndex((s) => s.query === query);
		return g.run(
			() => me.forgetSearches(query),
			async () => {
				await queryClient.invalidateQueries({ queryKey: ['recent-searches'] });
				// the next search takes the focus, else the field
				const next = at >= 0 ? document.getElementById(`${id}-${Math.min(at, shown.length - 1)}`) : null;
				await refocus(next, back);
			},
			query ?? ''
		);
	}
</script>

{#if shown.length}
	<section class="recent" aria-labelledby="{id}-title">
		<h2 id="{id}-title" class="label">Recent</h2>
		<ul class="plain-list">
			{#each shown as s, i (s.query)}
				<li>
					<button id="{id}-{i}" type="button" class="pill-btn" onclick={() => onsearch(s.query)}>
						<Icon name="clock" size={16} />{s.query}
					</button>
					<button
						type="button"
						class="icon-btn"
						aria-label="Forget “{s.query}”"
						{...pending(g.is(s.query))}
						onclick={() => forget(s.query)}
					>
						<Icon name="x" size={16} busy={g.is(s.query)} />
					</button>
				</li>
			{/each}
		</ul>
		<button type="button" class="link-btn clear" {...pending(g.is(''))} onclick={() => forget()}>Clear</button>
	</section>
{/if}

<style>
	.recent {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2) var(--s-3);
	}
	ul {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
	li {
		display: inline-flex;
		align-items: center;
	}
	.pill-btn {
		max-width: 16rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.clear {
		min-height: var(--control-h-xs);
	}
</style>
