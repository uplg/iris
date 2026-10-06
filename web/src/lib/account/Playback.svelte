<script lang="ts">
	// The languages playback starts in, on every device: the audio, and the subtitles (or
	// none). The player picks them when a file has no choice of its own; changing a track while
	// watching saves the new one here too. What is shown is what the server keeps.
	import { createQuery } from '@tanstack/svelte-query';
	import { me, type PlaybackPrefs } from '@iris/api/client';
	import { normalizeLang } from '@iris/core/subs/pick-subtitle';
	import { loadable, queryClient } from '#lib/query.ts';
	import { playbackPrefsSaved, read } from '#lib/queries.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { Gesture } from '#lib/gesture.svelte.ts';
	import Group from '#lib/components/Group.svelte';
	import Loaded from '#lib/components/Loaded.svelte';
	import Select from '#lib/components/Select.svelte';
	import { FILE_OWN, languageName, languagesPhrase, OFF, SUBTITLES_OFF } from '#lib/language.ts';

	const NONE = 'none';
	const COMMON = ['fr', 'en', 'es', 'de', 'it', 'pt', 'ja', 'ko'];
	const langName = (code: string) => languageName(code) ?? code;

	const prefs = createQuery(
		() => read.playbackPrefs(null),
		() => queryClient
	);
	const value = loadable(prefs);
	const g = new Gesture();

	/** A saved value as a choice: its ISO 639-1 base (« fre » and « fr-FR » are French). */
	const choice = (v: string | null | undefined) => (v === OFF ? OFF : (normalizeLang(v) ?? NONE));
	const audio = $derived(choice(prefs.data?.audio_language));
	const subtitles = $derived(choice(prefs.data?.subtitle_language));
	const listed = (current: string) => (current === NONE || current === OFF || COMMON.includes(current) ? COMMON : [...COMMON, current]);
	const audioOptions = $derived([{ value: NONE, label: FILE_OWN }, ...listed(audio).map((c) => ({ value: c, label: langName(c) }))]);
	const subtitleOptions = $derived([
		{ value: NONE, label: FILE_OWN },
		{ value: OFF, label: SUBTITLES_OFF },
		...listed(subtitles).map((c) => ({ value: c, label: langName(c) }))
	]);

	function save(field: 'audio_language' | 'subtitle_language', picked: string) {
		const body: PlaybackPrefs = {
			audio_language: prefs.data?.audio_language ?? null,
			subtitle_language: prefs.data?.subtitle_language ?? null,
			[field]: picked === NONE ? null : picked
		};
		return g.run(
			() => me.savePlaybackPreferences(body),
			async () => {
				await playbackPrefsSaved(null);
				ui.say(`Saved: ${languagesPhrase(body, true)}.`);
			},
			field
		);
	}
</script>

<Group id="playback-title" title="Playback">
	<p class="hint">The languages a film or an episode starts in, on every device, when it offers them.</p>
	<Loaded {value}>
		<div class="pick">
			<Select label="Audio" value={audio} options={audioOptions} onchange={(v) => save('audio_language', v)} />
			<Select label="Subtitles" value={subtitles} options={subtitleOptions} onchange={(v) => save('subtitle_language', v)} />
		</div>
	</Loaded>
</Group>

<style>
	.pick {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(14rem, 100%), 1fr));
		gap: var(--s-3);
		max-width: 32rem;
	}
</style>
