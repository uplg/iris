<script lang="ts">
	// The first visit: what one likes, for the suggestions. Open while the account's preferences
	// say onboarding is not done; both "Save" and "Skip for now" mark it done on the server, and
	// the sheet closes on the server's answer (its saved preferences replace the cached ones).
	// Closing it (Escape, the cross) skips for this visit and tells the server so too.
	import { createQuery } from '@tanstack/svelte-query';
	import { me, type Preferences } from '@iris/api/client';
	import Sheet from '#lib/components/Sheet.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import PreferencesEditor from '#lib/preferences/PreferencesEditor.svelte';
	import { picksOf, preferencesSaved, NO_PICKS, type Picks } from '#lib/preferences/preferences.ts';
	import { read } from '#lib/queries.ts';

	const prefs = createQuery(() => read.preferences());
	const due = $derived(prefs.data?.onboarding_completed === false);
	const genres = createQuery(() => ({ ...read.genres(), enabled: due }));
	const languages = createQuery(() => ({ ...read.languages(), enabled: due }));

	// what the person picked, from the saved preferences until they touch something
	let picked = $state<Picks | null>(null);
	const choice = $derived(picked ?? (prefs.data ? picksOf(prefs.data) : NO_PICKS));
	let later = $state(false);
	const g = new Gesture();
	const open = $derived(due && !later);

	function save(keep: boolean, inline = true) {
		// skipping keeps what the server already holds (picks made on the TV), only marks it done
		const body: Preferences = { ...(keep ? choice : prefs.data ? picksOf(prefs.data) : NO_PICKS), onboarding_completed: true };
		return g.run(
			() => me.savePreferences(body),
			(saved) => {
				preferencesSaved(saved);
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
			picks={choice}
			onchange={(next) => (picked = next)}
			languages={languages.data?.languages}
			genres={genres.data?.genres}
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
