<script lang="ts">
	// A tracker that did not answer: said where its releases would be, with a way to ask again.
	// The search endpoint asks every tracker at once, so asking again asks them all.
	import type { ProviderResultMeta } from '@iris/api/client';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';

	let { failed, retry }: { failed: readonly ProviderResultMeta[]; retry: () => Promise<unknown> } = $props();
	const g = new Gesture();
</script>

{#each failed as p (p.id)}
	<div class="banner warn" role="alert">
		<Icon name="triangle-alert" />
		<p>{p.id} did not answer, so its releases are missing from this list.</p>
		<button class="btn" {...pending(g.is(p.id))} onclick={() => g.run(retry, undefined, p.id)}>
			<Icon name="refresh-cw" busy={g.is(p.id)} />Retry {p.id}
		</button>
	</div>
{/each}

<style>
	.btn {
		min-height: var(--control-h);
	}
</style>
