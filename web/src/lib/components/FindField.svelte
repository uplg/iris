<script lang="ts">
	// A list's own find field: its label, the field, a Clear that empties it and gives the focus
	// back to it, and a hint under it when there is one. It grows in a row of filters.
	interface Props {
		/** The input's id: the callers give the focus back to it (`refocus`). */
		id: string;
		label: string;
		value: string;
		hint?: string;
	}
	let { id, label, value = $bindable(), hint }: Props = $props();
	let input = $state<HTMLInputElement>();

	function clear() {
		value = '';
		input?.focus();
	}
</script>

<div class="field find">
	<label for={id}>{label}</label>
	<div class="row">
		<input
			{id}
			bind:this={input}
			type="search"
			bind:value
			autocomplete="off"
			spellcheck="false"
			aria-describedby={hint ? `${id}-hint` : undefined}
		/>
		{#if value}<button class="btn ghost" onclick={clear}>Clear</button>{/if}
	</div>
	{#if hint}<span id="{id}-hint" class="hint">{hint}</span>{/if}
</div>

<style>
	.find {
		flex: 1 1 16rem;
	}
	.row {
		display: flex;
		gap: var(--s-2);
	}
	.row input {
		flex: 1;
		min-width: 0;
	}
	.row .btn {
		min-height: var(--control-h);
	}
</style>
