<script lang="ts">
	// The trackers (an admin's), read every 30 s: each one on or off in words, how its last
	// search went, and its switch. Off, a tracker is asked for nothing (a sick one no longer
	// slows every search) until it is on again; one disabled in providers.toml only the config
	// turns on. The switch shows what the server answered, never ahead of it.
	import { createQuery } from '@tanstack/svelte-query';
	import { admin, type ProviderStatus } from '@iris/api/client';
	import { onDay } from '@iris/api/format';
	import { loadable, queryClient } from '#lib/query.ts';
	import { Gesture } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ListRow from '#lib/components/ListRow.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import { providersQuery } from './queries.ts';

	const trackers = createQuery(providersQuery, () => queryClient);
	const value = loadable(trackers);
	const g = new Gesture();
	const reasonId = $props.id();

	const latency = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);
	const stateWords = (p: ProviderStatus) => (!p.configured ? 'Off in providers.toml' : p.enabled ? 'On' : 'Off · searches skip it');
	const lastSearch = (p: ProviderStatus) => {
		const s = p.last_search;
		if (!s || !p.enabled) return null;
		const outcome = s.error ? `failed after ${latency(s.latency_ms)}: ${s.error}` : `answered in ${latency(s.latency_ms)}`;
		return `Last search ${onDay(s.at)}: ${outcome}`;
	};
	const failing = (p: ProviderStatus) => p.enabled && !!p.last_search?.error;
	const summary = $derived.by(() => {
		const all = trackers.data ?? [];
		const on = all.filter((p) => p.enabled).length;
		const bad = all.filter(failing).length;
		return all.length ? `${on} of ${all.length} on${bad ? `, ${bad} failing` : ''}` : undefined;
	});
	const facts = (p: ProviderStatus) => [stateWords(p), p.kind !== p.id ? p.kind : null, lastSearch(p)].filter(Boolean).join(' · ');

	const toggle = (p: ProviderStatus, enabled: boolean) =>
		g.run(
			() => admin.setProviderEnabled(p.id, enabled),
			() => queryClient.invalidateQueries({ queryKey: providersQuery().queryKey }),
			p.id
		);
</script>

<Group id="trackers-title" title="Trackers" fact={summary}>
	<p class="hint">Turn off a tracker that is down: searches stop waiting for it. What it already downloaded keeps sharing.</p>
	<p id={reasonId} class="sr-only">Disabled in providers.toml: only the config can turn it on.</p>
	<Loaded {value} empty={trackers.data?.length === 0} emptyText="No trackers in providers.toml.">
		<ul class="plain-list">
			{#each trackers.data ?? [] as p (p.id)}
				<ListRow second={facts(p)}>
					<span class="name">{p.id}</span>
					{#if failing(p)}<span class="chip warn"><Icon name="triangle-alert" size={12} />Failing</span>{/if}
					{#snippet end()}
						<Toggle
							label={p.id}
							hideLabel
							checked={p.enabled}
							busy={g.is(p.id)}
							reason={p.configured ? undefined : reasonId}
							onchange={(next) => toggle(p, next)}
						/>
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
	.plain-list :global(.second) {
		overflow-wrap: anywhere;
	}
</style>
