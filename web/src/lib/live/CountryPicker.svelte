<script lang="ts">
	// The country whose channels show: one field (Bits UI Combobox, APG combobox with a listbox
	// popup). Typing narrows the list by name; opened untouched, the household's usual countries
	// come first, then every other one by name, each with how many channels it carries.
	import { Combobox } from 'bits-ui';
	import type { LiveCountry } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';
	import { channelCount, findCountries } from './guide.ts';

	interface Props {
		countries: readonly LiveCountry[];
		/** Shown first while nothing is typed. */
		usual: readonly LiveCountry[];
		value: string;
		onchange: (code: string) => void;
	}
	let { countries, usual, value, onchange }: Props = $props();
	const id = $props.id();

	const current = $derived(countries.find((c) => c.code === value));
	let open = $state(false);
	/** What was typed since the list opened, null before (the name shown is not a filter). */
	let typed = $state<string | null>(null);
	/** What the field shows: what is being typed, else the country's name. */
	const inputValue = $derived(typed ?? current?.name ?? '');

	const byName = $derived(countries.toSorted((a, b) => a.name.localeCompare(b.name, 'en')));
	const matches = $derived(typed ? findCountries(byName, typed) : []);
	const others = $derived.by(() => {
		const first = new Set(usual.map((c) => c.code));
		return byName.filter((c) => !first.has(c.code));
	});
	const items = $derived(countries.map((c) => ({ value: c.code, label: c.name })));
</script>

{#snippet option(c: LiveCountry)}
	{@const count = channelCount(c)}
	<Combobox.Item class="select-item country-item" value={c.code} label={c.name}>
		{#snippet children({ selected })}
			<span class="mark"
				>{#if selected}<Icon name="check" />{/if}</span
			>
			<span class="flag" aria-hidden="true">{c.flag}</span>
			<span class="country-name"
				>{c.name}{#if count}<span class="sr-only">,</span>{/if}</span
			>
			{#if count}<span class="count muted">{count}</span>{/if}
		{/snippet}
	</Combobox.Item>
{/snippet}

<div class="field country-field">
	<label class="label" for="{id}-input">Country</label>
	<Combobox.Root
		type="single"
		{value}
		{items}
		bind:open
		{inputValue}
		allowDeselect={false}
		onOpenChange={() => (typed = null)}
		onValueChange={(v) => {
			if (v && v !== value) onchange(v);
		}}
	>
		<div class="combo">
			{#if current && typed === null}<span class="flag current-flag" aria-hidden="true">{current.flag}</span>{/if}
			<Combobox.Input
				id="{id}-input"
				class="combo-input"
				placeholder="Type a country"
				autocomplete="off"
				aria-describedby="{id}-count"
				aria-controls="{id}-list"
				onfocus={(e) => e.currentTarget.select()}
				onclick={() => (open = true)}
				oninput={(e) => (typed = e.currentTarget.value)}
			/>
			<Combobox.Trigger class="combo-trigger" aria-label="Show every country"><Icon name="chevron-down" /></Combobox.Trigger>
		</div>
		<Combobox.Content class="select-content choice-content country-content" sideOffset={4} id="{id}-list" aria-label="Countries">
			{#if typed}
				{#each matches as c (c.code)}{@render option(c)}{/each}
				{#if !matches.length}<p class="none" role="status">No country matches “{typed}”.</p>{/if}
			{:else}
				{#if usual.length}
					<Combobox.Group>
						<Combobox.GroupHeading class="menu-heading">Your countries</Combobox.GroupHeading>
						{#each usual as c (c.code)}{@render option(c)}{/each}
					</Combobox.Group>
				{/if}
				<Combobox.Group>
					<Combobox.GroupHeading class="menu-heading">All countries</Combobox.GroupHeading>
					{#each others as c (c.code)}{@render option(c)}{/each}
				</Combobox.Group>
			{/if}
		</Combobox.Content>
	</Combobox.Root>
	<span class="sr-only" id="{id}-count">{current ? (channelCount(current) ?? '') : ''}</span>
</div>

<style>
	.combo {
		position: relative;
		display: flex;
		align-items: center;
	}
	.combo :global(.combo-input) {
		width: 100%;
		padding-inline: calc(var(--s-3) + 1.75rem) var(--control-h);
	}
	.current-flag {
		position: absolute;
		left: var(--s-3);
		pointer-events: none;
	}
	.combo :global(.combo-trigger) {
		position: absolute;
		right: 0;
		display: grid;
		place-items: center;
		width: var(--control-h);
		height: var(--control-h);
		border: 0;
		background: none;
		color: var(--ink-muted);
		cursor: pointer;
	}
	.flag {
		font-size: 1.25rem;
		line-height: 1;
	}
	:global(.country-content) {
		width: var(--bits-floating-anchor-width);
		max-height: min(26rem, var(--bits-floating-available-height));
		overflow-y: auto;
	}
	:global(.country-item) {
		min-height: var(--control-h);
	}
	.country-name {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.count {
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.mark {
		display: inline-grid;
		width: var(--check-box);
		flex: none;
		color: var(--accent);
	}
	.none {
		margin: 0;
		padding: var(--s-2) var(--s-3);
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
</style>
