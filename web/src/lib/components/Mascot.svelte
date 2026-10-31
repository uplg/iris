<script lang="ts">
	// A person's mascot (`@iris/api/mascot`): decoration beside their name, which always says it.
	import { MASCOT_BLUSH, mascot } from '@iris/api/mascot';

	let { name, size = 28 }: { name: string; size?: number } = $props();
	const m = $derived(mascot(name));
</script>

<svg class="mascot" viewBox="0 0 48 48" width={size} height={size} aria-hidden="true" style="--tile: var(--mascot-{m.tile})">
	<rect width="48" height="48" rx="11" fill="var(--tile)" />
	<path class="line" d={m.antenna} />
	<path d={m.screen} fill="var(--mascot-paper)" />
	<path class="eyes" d={m.eyes} />
	<path d={MASCOT_BLUSH} fill="var(--mascot-blush)" />
	<path class="line" d={m.foot} />
</svg>

<style>
	.mascot {
		flex: none;
		display: block;
	}
	.line,
	.eyes {
		fill: none;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.line {
		stroke: var(--mascot-paper);
		stroke-width: 2.4;
	}
	.eyes {
		stroke: var(--tile);
		stroke-width: 1.7;
	}
</style>
