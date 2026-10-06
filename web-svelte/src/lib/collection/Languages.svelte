<script lang="ts">
	// The languages the next episodes of this series start with, in words, and the way to change
	// them for this series alone (saved with its `collection_id`; the account-wide choice stays
	// for everything else). The player still lets each file pick its own track.
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
	import { languageName, prefsKey } from './lang.ts';

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
		() => ({ queryKey: prefsKey(collectionId), queryFn: () => me.seriesPlaybackPreferences(collectionId) }),
		() => queryClient
	);
	const audio = $derived(prefs.data?.audio_language ?? null);
	const subs = $derived(prefs.data?.subtitle_language ?? null);

	type Choice = { audio: string; subs: string };
	// '' is « no preference » (null on the wire): a radio needs a value
	const draft = new Draft<Choice>({ audio: '', subs: '' });
	let open = $state(false);
	let opener = $state<HTMLElement>();

	const options = (current: string | null, extra: string[] = []) => [
		...new Set([...known, 'en', 'fr', ...(current && current !== 'off' ? [current] : []), ...extra])
	];
	const audioOptions = $derived(options(audio));
	const subOptions = $derived(options(subs));

	function show() {
		draft.reset({ audio: audio ?? '', subs: subs ?? '' });
		open = true;
	}

	function close() {
		open = false;
		void refocus(opener);
	}

	const audioWords = (v: string | null) => (v ? languageName(v) : 'Each file’s own default');
	const subWords = (v: string | null) => (v === 'off' ? 'Off' : v ? languageName(v) : 'Each file’s own default');

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
				await queryClient.invalidateQueries({ queryKey: prefsKey(collectionId) });
				draft.reset(next);
				ui.toast(`Saved for ${title}: audio ${audioWords(next.audio || null)}, subtitles ${subWords(next.subs || null)}.`);
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
		<fieldset>
			<legend>Audio</legend>
			<div class="choices">
				<label class="pill-btn"><input type="radio" name="audio" value="" bind:group={draft.current.audio} />Each file’s own default</label>
				{#each audioOptions as code (code)}
					<label class="pill-btn"
						><input type="radio" name="audio" value={code} bind:group={draft.current.audio} />{languageName(code)}</label
					>
				{/each}
			</div>
		</fieldset>
		<fieldset>
			<legend>Subtitles</legend>
			<div class="choices">
				<label class="pill-btn"><input type="radio" name="subs" value="" bind:group={draft.current.subs} />Each file’s own default</label>
				<label class="pill-btn"><input type="radio" name="subs" value="off" bind:group={draft.current.subs} />Off</label>
				{#each subOptions as code (code)}
					<label class="pill-btn"><input type="radio" name="subs" value={code} bind:group={draft.current.subs} />{languageName(code)}</label
					>
				{/each}
			</div>
		</fieldset>
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
	fieldset {
		border: 0;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--s-2);
	}
	legend {
		font: var(--t-field-label);
		padding: 0;
		margin-bottom: var(--s-2);
	}
	.choices {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
	}
	.choices .pill-btn {
		min-height: var(--control-h);
	}
</style>
