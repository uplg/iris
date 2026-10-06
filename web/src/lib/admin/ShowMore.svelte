<script lang="ts">
	// The end of a long list: the next rows on a press (busy while they travel, the focus stays
	// on it), or, once everything is shown, how many there are in words.
	import Icon from '#lib/components/Icon.svelte';
	import { pending } from '#lib/gesture.svelte.ts';

	interface Props {
		/** More rows can be asked for. */
		more: boolean;
		busy?: boolean;
		/** The button's words (« Show more plays »). */
		label: string;
		/** Said once everything is shown, when the list was ever longer than a page. */
		done?: string;
		onmore: () => unknown;
	}
	let { more, busy = false, label, done, onmore }: Props = $props();
</script>

{#if more}
	<div class="more">
		<button class="btn" {...pending(busy)} onclick={() => !busy && onmore()}>
			<Icon name="chevron-down" {busy} />{label}
		</button>
	</div>
{:else if done}
	<p class="hint more">{done}</p>
{/if}

<style>
	.more {
		padding-block: var(--s-3);
	}
	.more .btn {
		min-height: var(--control-h);
	}
</style>
