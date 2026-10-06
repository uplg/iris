<script lang="ts">
	// Audio and subtitles, one panel (a non-modal dialog drawn inside the stage, so it stays
	// visible in full screen): two radio groups, each track named so two never read alike, a
	// pick applied at once. Escape or Close puts the focus back on the button that opened it.
	import type { AudioTrack, SubtitleTrack } from '@iris/core/manifest-client';
	import Icon from '#lib/components/Icon.svelte';
	import { audioLabels, subtitleLabels } from './tracks.ts';

	interface Props {
		audio: AudioTrack[];
		activeAudio: number;
		subtitles: SubtitleTrack[];
		activeSubtitle: number | null;
		onaudio: (index: number) => void;
		onsubtitle: (track: SubtitleTrack | null) => void;
		/** Where the choice is kept, in words (« Kept for the whole series »). */
		keptFor?: string;
		/** A page to style subtitles, when there is one. */
		styleHref?: string;
		onclose: () => void;
	}
	let { audio, activeAudio, subtitles, activeSubtitle, onaudio, onsubtitle, keptFor, styleHref, onclose }: Props = $props();
	const id = $props.id();
	const audioNames = $derived(audioLabels(audio));
	const subtitleNames = $derived(subtitleLabels(subtitles));
	let panel = $state<HTMLElement | null>(null);

	// the focus goes to the choice in place, so arrows move through the group at once
	$effect(() => {
		panel?.querySelector<HTMLInputElement>('input:checked')?.focus();
	});
</script>

<div
	bind:this={panel}
	class="stage-panel tracks"
	role="dialog"
	aria-labelledby="{id}-title"
	tabindex="-1"
	data-iris-chrome
	onkeydown={(e) => {
		if (e.key === 'Escape') {
			e.stopPropagation();
			onclose();
		}
	}}
>
	<div class="head">
		<h2 id="{id}-title" class="title">Audio and subtitles</h2>
		<button class="close" type="button" aria-label="Close" onclick={onclose}><Icon name="x" /></button>
	</div>
	<div class="groups">
		{#if audio.length > 1}
			<fieldset>
				<legend>Audio</legend>
				{#each audio as _, i (i)}
					<label class="choice">
						<input type="radio" name="{id}-audio" checked={i === activeAudio} onchange={() => onaudio(i)} />
						<span>{audioNames[i]}</span>
					</label>
				{/each}
			</fieldset>
		{/if}
		{#if subtitles.length > 0}
			<fieldset>
				<legend>Subtitles</legend>
				<label class="choice">
					<input type="radio" name="{id}-subs" checked={activeSubtitle === null} onchange={() => onsubtitle(null)} />
					<span>Off</span>
				</label>
				{#each subtitles as s, i (s.stream_idx)}
					<label class="choice">
						<input type="radio" name="{id}-subs" checked={activeSubtitle === s.stream_idx} onchange={() => onsubtitle(s)} />
						<span>{subtitleNames[i]}</span>
					</label>
				{/each}
			</fieldset>
		{/if}
	</div>
	{#if keptFor || styleHref}
		<p class="foot">
			{#if keptFor}<span>{keptFor}</span>{/if}
			{#if styleHref}<a href={styleHref}>Subtitle style</a>{/if}
		</p>
	{/if}
</div>

<style>
	.tracks {
		width: min(32rem, calc(100% - 2 * var(--s-3)));
	}
	/* a phone: a sheet across the bottom of the screen (IrisChrome) */
	@media (max-width: 599px) {
		.tracks {
			width: 100%;
		}
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-2);
	}
	.title {
		font: var(--t-group);
	}
	.close {
		display: grid;
		place-items: center;
		width: var(--control-h);
		height: var(--control-h);
		border: 0;
		border-radius: var(--radius);
		background: transparent;
		color: var(--stage-ink);
		cursor: pointer;
	}
	.close:hover {
		background: var(--stage-line);
	}
	.groups {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr));
		gap: var(--s-4);
		overflow-y: auto;
		min-height: 0;
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		display: grid;
		gap: var(--s-1);
		align-content: start;
	}
	legend {
		font: var(--t-field-label);
		color: var(--stage-muted);
		padding: 0 0 var(--s-1);
	}
	.choice {
		display: flex;
		align-items: center;
		gap: var(--s-3);
		min-height: var(--control-h);
		padding: 0 var(--s-2);
		border-radius: var(--radius);
		cursor: pointer;
	}
	.choice:hover {
		background: var(--stage-line);
	}
	.choice:has(input:checked) {
		color: var(--accent);
		font-weight: 600;
	}
	.choice input {
		width: var(--check-box);
		height: var(--check-box);
		margin: 0;
		accent-color: var(--accent);
		flex: none;
	}
	.foot {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: var(--s-2);
		font: var(--t-secondary);
		color: var(--stage-muted);
	}
</style>
