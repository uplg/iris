<script lang="ts" module>
	export interface MenuItem {
		label: string;
		run: () => unknown;
	}
</script>

<script lang="ts">
	// A card's actions, folded behind one button beside its name (Bits UI DropdownMenu, APG menu
	// button): named after the card ("More for Severance"), busy while one of them travels
	// (its items then do nothing: a second press never sends twice).
	import { DropdownMenu } from 'bits-ui';
	import Icon from '#lib/components/Icon.svelte';
	import { pending } from '#lib/gesture.svelte.ts';

	interface Props {
		label: string;
		items: readonly MenuItem[];
		busy?: boolean;
		/** The trigger, to find the row it sits in. */
		ref?: HTMLElement | null;
	}
	let { label, items, busy = false, ref = $bindable(null) }: Props = $props();
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class="icon-btn card-menu" aria-label={label} {...pending(busy)} bind:ref>
		<Icon name="ellipsis" {busy} />
	</DropdownMenu.Trigger>
	<DropdownMenu.Portal>
		<DropdownMenu.Content class="menu" align="end" sideOffset={4}>
			{#each items as item (item.label)}
				<DropdownMenu.Item class="menu-item" onSelect={() => !busy && item.run()}>{item.label}</DropdownMenu.Item>
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
	:global(.card-menu) {
		flex: none;
		width: var(--control-h);
		height: var(--control-h);
	}
</style>
