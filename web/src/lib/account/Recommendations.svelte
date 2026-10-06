<script lang="ts">
	// What « For You » is tuned by (PreferencesEditor), changed here and saved in one press;
	// whether there is something to save is said in words. What the server keeps is what shows
	// once saved.
	import { createQuery } from '@tanstack/svelte-query';
	import { me } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Draft } from '#lib/draft.svelte.ts';
	import { Gesture, pending, unavailable } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import PreferencesEditor from '#lib/preferences/PreferencesEditor.svelte';
	import { NO_PICKS, picksOf, preferencesSaved, type Picks } from '#lib/preferences/preferences.ts';
	import { read } from '#lib/queries.ts';

	const prefs = createQuery(read.preferences, () => queryClient);
	const genres = createQuery(read.genres, () => queryClient);
	const languages = createQuery(read.languages, () => queryClient);
	const value = loadable(prefs);
	const draft = new Draft<Picks>(NO_PICKS);
	const g = new Gesture();
	let seeded = false;

	$effect(() => {
		if (seeded || !prefs.data) return;
		seeded = true;
		draft.reset(picksOf(prefs.data));
	});

	function save() {
		if (!draft.dirty || !prefs.data) return;
		return g.run(
			() => me.savePreferences({ ...draft.current, onboarding_completed: prefs.data?.onboarding_completed ?? true }),
			(saved) => {
				preferencesSaved(saved);
				draft.reset(picksOf(saved));
				ui.toast('Recommendations saved.');
			},
			'save'
		);
	}
</script>

<Group id="reco-title" title="Recommendations">
	<p class="hint">What Home and Discover suggest first.</p>
	<Loaded {value}>
		<PreferencesEditor
			picks={draft.current}
			onchange={(next) => (draft.current = next)}
			languages={languages.data?.languages}
			genres={genres.data?.genres}
		/>
		<div class="actions">
			<button class="btn primary" {...pending(g.is('save'))} {...unavailable(!draft.dirty && 'reco-state')} onclick={save}>
				<Icon name="check" busy={g.is('save')} />Save recommendations
			</button>
			<span class="hint" id="reco-state">{draft.dirty ? 'You have changes not saved yet.' : 'Nothing changed since the last save.'}</span>
		</div>
	</Loaded>
</Group>

<style>
	.actions .btn {
		min-height: var(--control-h);
	}
</style>
