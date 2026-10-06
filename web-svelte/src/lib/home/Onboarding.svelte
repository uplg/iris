<script lang="ts">
	// The first visit: what one likes, for the suggestions. Open while the account's preferences
	// say onboarding is not done; both "Save" and "Skip for now" mark it done on the server, and
	// the sheet closes on the server's answer (its saved preferences replace the cached ones).
	// Closing it (Escape, the cross) skips for this visit and tells the server so too.
	import { createQuery } from '@tanstack/svelte-query';
	import { discover, me, type Preferences } from '@iris/api/client';
	import Sheet from '#lib/components/Sheet.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import PreferencesEditor from './PreferencesEditor.svelte';
	import { KEYS } from './data.ts';

	const DAY = 24 * 60 * 60_000;
	const prefs = createQuery(() => ({ queryKey: KEYS.preferences, queryFn: me.preferences, staleTime: 5 * 60_000 }));
	const due = $derived(prefs.data?.onboarding_completed === false);
	const genres = createQuery(() => ({ queryKey: KEYS.genres, queryFn: discover.genres, staleTime: DAY, enabled: due }));
	const languages = createQuery(() => ({ queryKey: KEYS.languages, queryFn: discover.languages, staleTime: DAY, enabled: due }));

	// what the person picked, from the saved preferences until they touch something
	let picked = $state<{ languages: string[]; genres: number[]; anime: boolean } | null>(null);
	const choice = $derived(
		picked ?? { languages: prefs.data?.languages ?? [], genres: prefs.data?.genres ?? [], anime: prefs.data?.include_anime ?? false }
	);
	let later = $state(false);
	const g = new Gesture();
	const open = $derived(due && !later);

	const toggle = <T>(list: T[], v: T) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
	const change = (next: Partial<typeof choice>) => (picked = { ...choice, ...next });

	function save(keep: boolean, inline = true) {
		const body: Preferences = {
			languages: keep ? choice.languages : [],
			genres: keep ? choice.genres : [],
			include_anime: keep && choice.anime,
			onboarding_completed: true
		};
		return g.run(
			() => me.savePreferences(body),
			(saved) => {
				queryClient.setQueryData(KEYS.preferences, saved);
				void queryClient.invalidateQueries({ queryKey: KEYS.forYou });
				ui.say(keep ? 'Your preferences are saved' : 'Skipped. You can set your preferences from your account.');
			},
			keep ? 'save' : 'skip',
			inline ? { inline: true } : {}
		);
	}

	function close() {
		later = true;
		void save(false, false);
	}
</script>

<Sheet
	{open}
	onclose={close}
	title="Personalize your home"
	description="Tell Iris what you are into and your suggestions follow. You can change this any time from your account."
>
	<div class="body">
		<PreferencesEditor
			languages={choice.languages}
			genres={choice.genres}
			includeAnime={choice.anime}
			languageOptions={languages.data?.languages ?? []}
			languagesLoading={languages.isPending}
			genreOptions={genres.data?.genres ?? []}
			genresLoading={genres.isPending}
			onToggleLanguage={(v) => change({ languages: toggle(choice.languages, v) })}
			onToggleGenre={(id) => change({ genres: toggle(choice.genres, id) })}
			onToggleAnime={() => change({ anime: !choice.anime })}
		/>
		<p class="form-error" role="alert">{g.error}</p>
		<div class="actions end">
			<button class="btn ghost" {...pending(g.is('skip'))} onclick={() => save(false)}>
				{#if g.is('skip')}<Icon name="loader-circle" busy />{/if}Skip for now
			</button>
			<button class="btn primary" {...pending(g.is('save'))} onclick={() => save(true)}>
				{#if g.is('save')}<Icon name="loader-circle" busy />{/if}{g.is('save') ? 'Saving…' : 'Save preferences'}
			</button>
		</div>
	</div>
</Sheet>

<style>
	.body {
		display: grid;
		gap: var(--s-5);
	}
	.body .btn {
		min-height: var(--control-h);
	}
</style>
