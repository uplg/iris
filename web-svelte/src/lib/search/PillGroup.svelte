<script lang="ts" generics="T">
	// A filter: its name, then its choices as pills, the chosen one pressed (`aria-pressed`, and
	// drawn filled: never color alone). Each pill may carry a count (what the loaded page holds).
	import Icon from '#lib/components/Icon.svelte';

	interface Props {
		legend: string;
		options: readonly { value: T; label: string; count?: number }[];
		value: T;
		onchange: (v: T) => void;
	}
	let { legend, options, value, onchange }: Props = $props();
</script>

<fieldset class="pills">
	<legend>{legend}</legend>
	<div class="row">
		{#each options as o (o.value ?? '')}
			<button
				type="button"
				class={['pill-btn', o.value === value && 'on']}
				aria-pressed={o.value === value}
				onclick={() => onchange(o.value)}
			>
				{#if o.value === value}<Icon name="check" size={16} />{/if}{o.label}
				{#if o.count !== undefined}<span class="count">{o.count}</span>{/if}
			</button>
		{/each}
	</div>
</fieldset>

<style>
	.pills {
		border: 0;
		margin: 0;
		padding: 0;
		min-width: 0;
		display: grid;
		gap: var(--s-2);
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
	.count {
		font: var(--t-tiny);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
</style>
