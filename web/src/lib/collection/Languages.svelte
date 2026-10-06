<script lang="ts">
	// The languages the next episodes of this series start with, in words, and the way to change
	// them for this series alone (saved with its `collection_id`; the account-wide choice stays
	// for everything else). The sheet holds the series' own choices: a field left on « your usual
	// choice » saves as null and keeps inheriting the account's, never a copy of it. The player
	// still lets each file pick its own track.
	import { createQuery } from '@tanstack/svelte-query';
	import { me } from '@iris/api/client';
	import { queryClient } from '#lib/query.ts';
	import { Draft } from '#lib/draft.svelte.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import StatusRow from '#lib/components/StatusRow.svelte';
	import PillChoice from '#lib/components/PillChoice.svelte';
	import { playbackPrefsSaved, read } from '#lib/queries.ts';
	import { languageName } from '#lib/language.ts';
	import { ownChoices } from '#lib/watch/prefs.ts';

	interface Props {
		collectionId: string;
		title: string;
		/** Languages worth offering first: the releases', the original one. */
		known: string[];
	}
	let { collectionId, title, known }: Props = $props();
	const id = $props.id();
	const g = new Gesture();

	const prefs = createQuery(
		() => read.playbackPrefs(collectionId),
		() => queryClient
	);
	const audio = $derived(prefs.data?.audio_language ?? null);
	const subs = $derived(prefs.data?.subtitle_language ?? null);
	const own = $derived(ownChoices(prefs.data, collectionId));

	type Choice = { audio: string; subs: string };
	// '' is « your usual choice » (null on the wire: inherits the account's): a radio needs a value
	const draft = new Draft<Choice>({ audio: '', subs: '' });
	let open = $state(false);
	let opener = $state<HTMLElement>();

	const options = (current: string | null, extra: string[] = []) => [
		...new Set([...known, 'en', 'fr', ...(current && current !== 'off' ? [current] : []), ...extra])
	];
	const audioOptions = $derived(options(audio));
	const subOptions = $derived(options(subs));

	function show() {
		draft.reset({ audio: own.audio_language ?? '', subs: own.subtitle_language ?? '' });
		open = true;
	}

	function close() {
		open = false;
		void refocus(opener);
	}

	const audioWords = (v: string | null) => (v ? (languageName(v) ?? v) : 'Each file’s own default');
	const subWords = (v: string | null) => (v === 'off' ? 'Off' : v ? (languageName(v) ?? v) : 'Each file’s own default');

	const USUAL = 'your usual choice';
	const chosenWords = (v: string, words: (v: string | null) => string) => (v ? words(v) : USUAL);

	function save(e: SubmitEvent) {
		e.preventDefault();
		const next = draft.current;
		void g.run(
			() =>
				me.savePlaybackPreferences({
					audio_language: next.audio || null,
					subtitle_language: next.subs || null,
					collection_id: collectionId
				}),
			async () => {
				await playbackPrefsSaved(collectionId);
				draft.reset(next);
				ui.toast(`Saved for ${title}: audio ${chosenWords(next.audio, audioWords)}, subtitles ${chosenWords(next.subs, subWords)}.`);
				close();
			},
			'save',
			{ inline: true }
		);
	}
</script>

<section class="tile langs" aria-labelledby="{id}-title">
	<h2 id="{id}-title" class="group-title">Next episodes play with</h2>
	{#if prefs.isPending}
		<p class="hint">Loading…</p>
	{:else}
		<dl class="facts">
			<StatusRow icon="volume-2" label="Audio" value={audioWords(audio)} />
			<StatusRow icon="captions" label="Subtitles" value={subWords(subs)} />
		</dl>
		<p class="hint">{prefs.data?.for_collection ? 'Chosen for this series.' : 'Your usual choice, from your account.'}</p>
	{/if}
	<button class="btn" bind:this={opener} onclick={show}><Icon name="languages" />Change languages</button>
</section>

<Sheet
	{open}
	onclose={close}
	title="Languages for {title}"
	description="The next episodes start with these. Other titles keep your usual choice."
	{draft}
>
	<form class="form" onsubmit={save}>
		<PillChoice
			legend="Audio"
			options={[
				{ value: '', label: 'Your usual choice' },
				...audioOptions.map((code) => ({ value: code, label: languageName(code) ?? code }))
			]}
			value={draft.current.audio}
			onchange={(v) => (draft.current.audio = v)}
		/>
		<PillChoice
			legend="Subtitles"
			options={[
				{ value: '', label: 'Your usual choice' },
				{ value: 'off', label: 'Off' },
				...subOptions.map((code) => ({ value: code, label: languageName(code) ?? code }))
			]}
			value={draft.current.subs}
			onchange={(v) => (draft.current.subs = v)}
		/>
		<p class="form-error" role="alert">{g.error}</p>
		<div class="actions end">
			<button type="button" class="btn" onclick={close}>Cancel</button>
			<button type="submit" class="btn primary" {...pending(g.is('save'))}
				><Icon name="check" busy={g.is('save')} />Save for this series</button
			>
		</div>
	</form>
</Sheet>

<style>
	.langs {
		align-content: start;
	}
	.langs .btn,
	.form .btn {
		min-height: var(--control-h);
	}
	.form {
		display: grid;
		gap: var(--s-5);
	}
</style>
