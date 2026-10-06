<script lang="ts">
	// A share as a bar, beside the words that already say it: hidden from assistive tech then.
	// Given a `label`, it is a progressbar of its own, named by it, its value in words.
	// `--meter-ground` and `--meter-max` let a surface pick its track colour and width.
	import { percent } from '@iris/api/format';

	interface Props {
		/** 0 to 1. */
		share: number;
		label?: string;
		thin?: boolean;
	}
	let { share, label, thin = false }: Props = $props();
	const pct = $derived(Math.round(Math.min(1, Math.max(0, share)) * 100));
</script>

{#if label}
	<span
		class="meter"
		class:thin
		role="progressbar"
		aria-label={label}
		aria-valuemin={0}
		aria-valuemax={100}
		aria-valuenow={pct}
		aria-valuetext={percent(pct)}><span style:width="{pct}%"></span></span
	>
{:else}
	<span class="meter" class:thin aria-hidden="true"><span style:width="{pct}%"></span></span>
{/if}

<style>
	.meter {
		display: block;
		height: var(--track-h);
		max-width: var(--meter-max, none);
		border-radius: var(--radius-pill);
		background: var(--meter-ground, var(--ground-raised));
		overflow: hidden;
	}
	.thin {
		height: var(--s-1);
	}
	.meter span {
		display: block;
		height: 100%;
		background: var(--accent);
	}
</style>
