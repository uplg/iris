<script lang="ts" generics="T">
	// One choice among a few, as pills: a radio group (one Tab stop, arrows between them), the
	// chosen pill filled and ticked, never color alone. A pill may carry a count (what the
	// loaded page holds).
	import Icon from './Icon.svelte';

	interface Props {
		legend: string;
		options: readonly { value: T; label: string; count?: number }[];
		value: T;
		onchange: (v: T) => void;
	}
	let { legend, options, value, onchange }: Props = $props();
	const id = $props.id();
</script>

<fieldset class="pill-choice">
	<legend>{legend}</legend>
	<div class="row">
		{#each options as o, i (i)}
			{@const on = o.value === value}
			<label class="pill-btn">
				<input class="sr-only" type="radio" name={id} value={i} checked={on} onchange={() => onchange(o.value)} />
				{#if on}<Icon name="check" size={16} />{/if}{o.label}
				{#if o.count !== undefined}<span class="count">{o.count}</span>{/if}
			</label>
		{/each}
	</div>
</fieldset>

<style>
	.pill-choice {
		border: 0;
		margin: 0;
		padding: 0;
		min-width: 0;
	}
	legend {
		font: var(--t-field-label);
		padding: 0;
		margin-bottom: var(--s-2);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
	.pill-btn {
		min-height: var(--control-h);
	}
	.pill-btn:has(:focus-visible) {
		outline: var(--focus-ring);
		outline-offset: var(--focus-offset);
	}
	.count {
		font: var(--t-tiny);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
</style>
