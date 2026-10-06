<script lang="ts">
	// What « For You » is tuned by, picked by pressing (each choice a toggle button that says
	// whether it is on, a tick for the eye): languages most wanted, genres enjoyed, and Anime,
	// its own category (not TMDB's Animation). The choices come from the server. Stateless:
	// Account's Recommendations and the first-run onboarding own the picks and their saving.
	import type { GenreOption, LanguageOption } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';
	import { toggled, type Picks } from './preferences.ts';

	interface Props {
		picks: Picks;
		onchange: (next: Picks) => void;
		languages: readonly LanguageOption[] | undefined;
		genres: readonly GenreOption[] | undefined;
	}
	let { picks, onchange, languages, genres }: Props = $props();
	const id = $props.id();
</script>

{#snippet pill(on: boolean, label: string, press: () => void)}
	<button type="button" class="pill-btn" class:on aria-pressed={on} onclick={press}>
		{#if on}<Icon name="check" size={14} />{/if}{label}
	</button>
{/snippet}

<div class="editor">
	<div class="choice" role="group" aria-labelledby="{id}-lang" aria-describedby="{id}-lang-hint">
		<p class="label" id="{id}-lang">Languages</p>
		<p class="hint" id="{id}-lang-hint">What you would rather watch in. Releases in these come first.</p>
		<div class="pills">
			{#if languages}
				{#each languages as l (l.value)}
					{@render pill(picks.languages.includes(l.value), l.label, () =>
						onchange({ ...picks, languages: toggled(picks.languages, l.value) })
					)}
				{/each}
			{:else}
				<p class="hint">Loading the languages…</p>
			{/if}
		</div>
	</div>
	<div class="choice" role="group" aria-labelledby="{id}-genres" aria-describedby="{id}-genres-hint">
		<p class="label" id="{id}-genres">Genres</p>
		<p class="hint" id="{id}-genres-hint">
			Pick a few you enjoy, or none for a bit of everything. Anime is its own category, apart from Animation.
		</p>
		<div class="pills">
			{@render pill(picks.include_anime, 'Anime', () => onchange({ ...picks, include_anime: !picks.include_anime }))}
			{#if genres}
				{#each genres as gen (gen.id)}
					{@render pill(picks.genres.includes(gen.id), gen.name, () => onchange({ ...picks, genres: toggled(picks.genres, gen.id) }))}
				{/each}
			{:else}
				<p class="hint">Loading the genres…</p>
			{/if}
		</div>
	</div>
</div>

<style>
	.editor {
		display: grid;
		gap: var(--s-5);
	}
	.choice {
		display: grid;
		gap: var(--s-2);
	}
	.choice p {
		margin: 0;
	}
	.pills {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
</style>
