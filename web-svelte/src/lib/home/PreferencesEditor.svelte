<script lang="ts">
	// What a person likes, for the suggestions: the languages they would rather watch in (the
	// server's list, so a new language needs no client release), and genres, Anime among them
	// as its own category (not TMDB's Animation). Each choice a toggle button that says whether
	// it is on (`aria-pressed`, and a tick: never color alone). Stateless: the caller keeps the
	// choice and saves it.
	import type { GenreOption, LanguageOption } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';

	interface Props {
		languages: readonly string[];
		genres: readonly number[];
		includeAnime: boolean;
		languageOptions: readonly LanguageOption[];
		languagesLoading?: boolean;
		genreOptions: readonly GenreOption[];
		genresLoading?: boolean;
		onToggleLanguage: (value: string) => void;
		onToggleGenre: (id: number) => void;
		onToggleAnime: () => void;
	}
	let {
		languages,
		genres,
		includeAnime,
		languageOptions,
		languagesLoading = false,
		genreOptions,
		genresLoading = false,
		onToggleLanguage,
		onToggleGenre,
		onToggleAnime
	}: Props = $props();
	const id = $props.id();
</script>

<div class="prefs">
	<fieldset aria-describedby="{id}-lang-hint">
		<legend class="label">Languages</legend>
		<p class="hint" id="{id}-lang-hint">What you would rather watch in. Releases in these come first.</p>
		{#if languagesLoading}
			<p class="hint">Loading languages…</p>
		{:else}
			<div class="choices">
				{#each languageOptions as l (l.value)}
					{@render choice(l.label, languages.includes(l.value), () => onToggleLanguage(l.value))}
				{/each}
			</div>
		{/if}
	</fieldset>
	<fieldset aria-describedby="{id}-genre-hint">
		<legend class="label">Genres</legend>
		<p class="hint" id="{id}-genre-hint">
			Pick a few you enjoy. Anime is its own category, apart from Animation. Leave all off for a bit of everything.
		</p>
		<div class="choices">
			{@render choice('Anime', includeAnime, onToggleAnime)}
			{#if genresLoading}
				<span class="hint">Loading genres…</span>
			{:else}
				{#each genreOptions as g (g.id)}
					{@render choice(g.name, genres.includes(g.id), () => onToggleGenre(g.id))}
				{/each}
			{/if}
		</div>
	</fieldset>
</div>

{#snippet choice(label: string, on: boolean, toggle: () => void)}
	<button type="button" class="pill-btn" class:on aria-pressed={on} onclick={toggle}>
		{#if on}<Icon name="check" size={16} />{/if}{label}
	</button>
{/snippet}

<style>
	.prefs {
		display: grid;
		gap: var(--s-5);
	}
	fieldset {
		display: grid;
		gap: var(--s-2);
		margin: 0;
		padding: 0;
		border: 0;
		min-width: 0;
	}
	legend {
		padding: 0;
		margin-bottom: var(--s-1);
	}
	.choices {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
	.choices .pill-btn {
		min-height: var(--control-h-s);
	}
</style>
