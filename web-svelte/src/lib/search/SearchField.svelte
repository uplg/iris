<script lang="ts">
	// The search field: what one types, TMDB's titles offered under it as one types (APG combobox
	// with a listbox: arrows go through them, Enter picks the highlighted one, Escape closes), and
	// the Search button. Trackers are asked on submit only; TMDB a moment after the last key.
	import { createQuery } from '@tanstack/svelte-query';
	import { metadata, tmdbImage, type TmdbSuggestion } from '@iris/api/client';
	import { queryClient } from '#lib/query.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { kindWord } from './release.ts';
	import { Settled } from './typeahead.svelte.ts';

	interface Props {
		value: string;
		onsubmit: (q: string) => void;
		onpick: (s: TmdbSuggestion) => void;
		/** Ids of what describes the field (the parsed-query hint). */
		describedby?: string;
		input?: HTMLInputElement;
	}
	let { value = $bindable(), onsubmit, onpick, describedby, input = $bindable() }: Props = $props();
	const id = $props.id();
	// svelte-ignore state_referenced_locally - the field's first value; then it follows the typing
	const typed = new Settled(value.trim());
	let open = $state(false);
	let active = $state(-1);

	$effect(() => () => typed.stop());

	const suggest = createQuery(
		() => ({
			queryKey: ['tmdb-suggest', typed.value],
			queryFn: () => metadata.tmdbSearch(typed.value),
			enabled: open && typed.value.length >= 2,
			staleTime: 60_000
		}),
		() => queryClient
	);
	const items = $derived(open && typed.value.length >= 2 ? (suggest.data ?? []).slice(0, 8) : []);
	const expanded = $derived(items.length > 0);
	const optionId = (i: number) => `${id}-option-${i}`;

	function oninput(e: Event & { currentTarget: HTMLInputElement }) {
		value = e.currentTarget.value;
		open = true;
		active = -1;
		const q = value.trim();
		if (q.length < 2) typed.now(q);
		else typed.set(q);
	}

	function pick(s: TmdbSuggestion) {
		value = s.title;
		typed.now(s.title);
		open = false;
		active = -1;
		onpick(s);
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			if (!expanded) {
				open = true;
				return;
			}
			e.preventDefault();
			const step = e.key === 'ArrowDown' ? 1 : -1;
			active = (active + step + items.length + 1) % (items.length + 1);
			if (active === items.length) active = -1;
		} else if (e.key === 'Enter' && expanded && active >= 0) {
			e.preventDefault();
			pick(items[active]);
		} else if (e.key === 'Escape' && expanded) {
			e.preventDefault();
			open = false;
			active = -1;
		}
	}

	function submit(e: SubmitEvent) {
		e.preventDefault();
		open = false;
		active = -1;
		onsubmit(value.trim());
	}
</script>

<form class="search" role="search" onsubmit={submit}>
	<div class="field">
		<label for="{id}-input">Title, year or release name</label>
		<div class="line">
			<div class="box">
				<input
					id="{id}-input"
					bind:this={input}
					type="search"
					{value}
					role="combobox"
					autocomplete="off"
					spellcheck="false"
					enterkeyhint="search"
					aria-autocomplete="list"
					aria-expanded={expanded}
					aria-controls="{id}-list"
					aria-activedescendant={expanded && active >= 0 ? optionId(active) : undefined}
					aria-describedby={describedby}
					{oninput}
					{onkeydown}
					onfocus={() => (open = true)}
					onblur={() => {
						open = false;
						active = -1;
					}}
				/>
				<ul id="{id}-list" class="suggestions select-content" role="listbox" aria-label="Titles on TMDB" hidden={!expanded}>
					{#each items as s, i (`${s.kind}-${s.tmdb_id}`)}
						<!-- the pointer picks without taking the focus from the field; the keyboard stays in the field
						     (arrows, Enter: APG combobox), so the option needs no key handler of its own -->
						<!-- svelte-ignore a11y_click_events_have_key_events -->
						<li
							id={optionId(i)}
							role="option"
							class="select-item"
							aria-selected={i === active}
							data-highlighted={i === active ? '' : undefined}
							onpointerdown={(e) => e.preventDefault()}
							onclick={() => pick(s)}
						>
							{#if s.poster_path}
								<img src={tmdbImage(s.poster_path, 'w92')} alt="" width="32" height="48" loading="lazy" decoding="async" />
							{:else}
								<span class="thumb"><Icon name={s.kind === 'tv' ? 'tv' : 'film'} size={16} /></span>
							{/if}
							<span class="text">
								<span class="name">{s.title}</span>
								<span class="meta">{[kindWord(s.kind), s.year].filter(Boolean).join(' · ')}</span>
							</span>
						</li>
					{/each}
				</ul>
			</div>
			<button class="btn primary go" type="submit"><Icon name="search" />Search</button>
		</div>
	</div>
</form>

<style>
	.search {
		max-width: var(--measure);
	}
	.line {
		display: flex;
		gap: var(--s-2);
	}
	.box {
		position: relative;
		flex: 1;
		min-width: 0;
	}
	input {
		width: 100%;
	}
	.go {
		min-height: var(--control-h);
		flex: none;
	}
	.suggestions {
		position: absolute;
		top: calc(100% + var(--s-1));
		left: 0;
		right: 0;
		margin: 0;
		list-style: none;
		max-height: 24rem;
		overflow-y: auto;
	}
	.select-item {
		min-height: var(--control-h);
	}
	img,
	.thumb {
		flex: none;
		width: 32px;
		height: 48px;
		border-radius: var(--radius-s);
		object-fit: cover;
		background: var(--ground-raised);
	}
	.thumb {
		display: grid;
		place-items: center;
		color: var(--ink-muted);
	}
	.text {
		display: grid;
		min-width: 0;
	}
	.name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.meta {
		font: var(--t-meta);
		color: var(--ink-muted);
	}
</style>
