<script lang="ts">
	// The films on SéléoTV, the household's other app (an admin's), newest share first: until
	// when, who shared it, and the two gestures, a week more or stop now. A film is shared from its
	// title's page or from the releases list; read every minute.
	import { createQuery } from '@tanstack/svelte-query';
	import { admin, type SeleoShare } from '@iris/api/client';
	import { formatSize, onDay, plural } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import { seleoQuery } from './queries.ts';

	const shares = createQuery(seleoQuery, () => queryClient);
	const value = loadable(shares);
	const g = new Gesture();

	const day = (iso: string) => onDay(iso).replace(/^on /, '');
	const key = (s: SeleoShare) => `${s.infohash}:${s.file_idx}`;
	const name = (s: SeleoShare) => (s.year ? `${s.title} (${s.year})` : s.title);
	const facts = (s: SeleoShare) =>
		[`Until ${day(s.until)}`, s.shared_by_name && `shared by ${s.shared_by_name}`, formatSize(s.size_bytes)].filter(Boolean).join(' · ');
	const reread = () => queryClient.invalidateQueries({ queryKey: seleoQuery().queryKey });

	const extend = (s: SeleoShare) =>
		g.run(
			() => admin.shareWithSeleo(s.infohash, s.file_idx, 7),
			async (next) => {
				await reread();
				ui.toast(`${s.title} is on SéléoTV until ${day(next.until)}.`);
			},
			`extend:${key(s)}`
		);
	const stop = (s: SeleoShare) =>
		g.run(
			() => admin.unshareFromSeleo(s.infohash, s.file_idx),
			async () => {
				await reread();
				ui.toast(`${s.title} is no longer on SéléoTV.`);
			},
			`stop:${key(s)}`
		);
</script>

<Group id="seleo-title" title="SéléoTV" fact={shares.data?.length ? plural(shares.data.length, 'film') : undefined}>
	<p class="hint">Films shared with SéléoTV. Shared, a film stays on disk until its share ends.</p>
	<Loaded {value} empty={shares.data?.length === 0} emptyText="No film on SéléoTV.">
		<ul class="plain-list">
			{#each shares.data ?? [] as s (key(s))}
				<ListRow second={facts(s)}>
					{#if s.collection_id}<a class="name" href="/collection/{s.collection_id}">{name(s)}</a>{:else}<span class="name">{name(s)}</span
						>{/if}
					{#snippet end()}
						<button
							class="btn ghost"
							aria-label="Extend {s.title} on SéléoTV by a week"
							{...pending(g.is(`extend:${key(s)}`))}
							onclick={() => extend(s)}
						>
							<Icon name="plus" busy={g.is(`extend:${key(s)}`)} />Extend
						</button>
						<button
							class="btn ghost"
							aria-label="Stop sharing {s.title} with SéléoTV"
							{...pending(g.is(`stop:${key(s)}`))}
							onclick={() => stop(s)}
						>
							<Icon name="x" busy={g.is(`stop:${key(s)}`)} />Stop sharing
						</button>
					{/snippet}
				</ListRow>
			{/each}
		</ul>
	</Loaded>
</Group>

<style>
	.name {
		overflow-wrap: anywhere;
		min-width: 0;
		font-weight: 600;
	}
	.btn {
		min-height: var(--control-h);
	}
</style>
