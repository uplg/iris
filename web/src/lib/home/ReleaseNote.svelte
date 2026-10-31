<script lang="ts">
	// What changed, in a sentence, once per browser after an update. A browser that never saw a
	// version only remembers this one: a first visit has nothing to catch up on.
	import { createQuery } from '@tanstack/svelte-query';
	import Icon from '#lib/components/Icon.svelte';
	import { read } from '#lib/queries.ts';
	import { STORAGE } from '#lib/storage.ts';
	import { json, stored } from '#lib/stored.ts';
	import { noteDue } from './data.ts';

	const isText = (v: unknown): v is string => typeof v === 'string';
	const kept = stored<string | undefined>(STORAGE.releaseSeen, undefined, json(isText));
	const seen = kept.get();

	const release = createQuery(() => read.releaseNote());
	let closed = $state(false);
	const note = $derived(release.data && !closed ? noteDue(release.data, seen) : undefined);

	$effect(() => {
		if (release.data && !note) kept.set(release.data.version);
	});
</script>

{#if note && release.data}
	<p class="note">
		<span>Updated to {release.data.version} · {note}</span>
		<button class="close" aria-label="Close" onclick={() => (closed = true)}><Icon name="x" /></button>
	</p>
{/if}

<style>
	.note {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-2);
		margin: 0;
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.close {
		display: inline-grid;
		flex: none;
		place-items: center;
		min-width: var(--control-h);
		min-height: var(--control-h);
		padding: 0;
		border: 0;
		background: none;
		color: inherit;
		cursor: pointer;
	}
</style>
