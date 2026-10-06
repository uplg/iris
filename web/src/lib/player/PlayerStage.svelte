<script lang="ts" module>
	import type { Snippet } from 'svelte';
	import type { EngineHandle } from '@iris/core/engine';
	import type { Manifest, SubtitleTrack } from '@iris/core/manifest-client';
	import type { MediaState } from './media-state.svelte.ts';
	import type { ChromePip, NextEpisode } from './IrisChrome.svelte';

	/** What the stage draws, read live from IrisPlayer (getters): the same object feeds the stage
	 * in the page and the one mounted into a picture-in-picture window. */
	export interface StageView {
		readonly videoHost: HTMLElement;
		readonly handle: EngineHandle | null;
		readonly media: MediaState;
		readonly manifest: Manifest;
		readonly live: boolean;
		readonly title: string;
		readonly activeSubtitle: SubtitleTrack | null;
		readonly activeOverlay: SubtitleTrack | null;
		readonly activeAudioIndex: number;
		readonly cueGuard: boolean;
		readonly pip: ChromePip;
		readonly nextEpisode: NextEpisode | null;
		readonly debugInfo: [string, string][];
		readonly top: Snippet | undefined;
		readonly keptFor: string | undefined;
		/** A problem to say on the stage, in words (the server unreachable, a player error). */
		readonly notice: string | null;
		readonly controlsVisible: boolean;
		/** A picture-in-picture window holds the playing stage. */
		readonly pipOpen: boolean;
		getCurrentTime(): number;
		onSubtitlePick(track: SubtitleTrack | null): void;
		onAudioPick(id: string): void;
		onVolumeChange(volume: number, muted: boolean): void;
		onControlsVisible(visible: boolean): void;
		say(text: string): void;
	}

	/** Under this many ms, a second click is a double click (most systems' threshold). */
	export const DOUBLE_CLICK_MS = 250;
</script>

<script lang="ts">
	// The dark stage: the engine's picture (its host element re-attached here, in whichever
	// document this stage lives), the cue guard, the waiting spinner, the ASS/PGS overlay and the
	// chrome. A click toggles playback, a double click toggles full screen.
	import Icon from '#lib/components/Icon.svelte';
	import IrisChrome from './IrisChrome.svelte';
	import SubtitleOverlay from './SubtitleOverlay.svelte';
	import { toggleFullscreen } from './browser.ts';

	let { view, place = 'page' }: { view: StageView; place?: 'page' | 'pip' } = $props();
	let wrapper = $state<HTMLDivElement | null>(null);
	/** The stage that plays: the PiP one while that window is open, else the page's. */
	const active = $derived(place === 'pip' || !view.pipOpen);

	// the engine's host joins the playing stage: moved straight from one document to the other
	// (adopted, never detached on the way, so the media element keeps playing)
	function slot(el: HTMLElement) {
		if (active && view.videoHost.parentElement !== el) el.appendChild(view.videoHost);
	}

	/** A click on a control (or anything marked as chrome) is that control's, not the video's. */
	function onControl(e: MouseEvent): boolean {
		let n = e.target as HTMLElement | null;
		while (n && n !== e.currentTarget) {
			if (/^(BUTTON|INPUT|A|SELECT|TEXTAREA|LABEL)$/.test(n.tagName) || n.dataset.irisChrome !== undefined) return true;
			n = n.parentElement;
		}
		return false;
	}

	// approved web timer: single click (play/pause) vs double click (full screen); the browser
	// fires `click` for both clicks of a double click, so the toggle waits DOUBLE_CLICK_MS
	let clickTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => () => clearTimeout(clickTimer));
	function click(e: MouseEvent) {
		if (onControl(e)) return;
		// this press was the chrome's (IrisChrome: it closed a panel, or a tap brought the hidden
		// controls back): it does not also toggle playback
		if (wrapper?.dataset.pressConsumed !== undefined) {
			delete wrapper.dataset.pressConsumed;
			return;
		}
		const h = view.handle;
		if (!h) return;
		clearTimeout(clickTimer);
		clickTimer = setTimeout(() => {
			clickTimer = undefined;
			if (h.paused()) void h.play().catch(() => undefined);
			else h.pause();
			view.media.read(h);
		}, DOUBLE_CLICK_MS);
	}
	function dblclick(e: MouseEvent) {
		if (onControl(e)) return;
		clearTimeout(clickTimer);
		clickTimer = undefined;
		void toggleFullscreen(wrapper);
	}

	const status = $derived(view.notice ?? (view.media.busy ? 'Buffering' : ''));
</script>

<!-- the keys are the chrome's (IrisChrome listens on this element); a click here is the video's -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div bind:this={wrapper} class="stage" class:still={!view.controlsVisible} onclick={click} ondblclick={dblclick}>
	<div class="slot" {@attach slot}></div>
	{#if view.cueGuard}
		<!-- Windows + Chromium with a native subtitle showing: with nothing painted over it the
		     video may be promoted to a hardware overlay plane a few seconds after the chrome
		     fades, and buggy driver paths then swallow the browser's own cue boxes (Edge +
		     HDR/DV HEVC). A 1 %-alpha layer keeps the video compositor-composed. -->
		<div class="cue-guard" aria-hidden="true"></div>
	{/if}
	{#if view.media.busy && !view.notice}
		<div class="busy" aria-hidden="true"><Icon name="loader-circle" size={44} busy /></div>
	{/if}
	<p class="status" class:said={!!view.notice} role="status">{status}</p>
	<SubtitleOverlay host={active ? wrapper : null} track={view.activeOverlay} getCurrentTime={() => view.getCurrentTime()} />
	<IrisChrome
		handle={view.handle}
		media={view.media}
		manifest={view.manifest}
		live={view.live}
		activeSubtitle={view.activeSubtitle}
		onSubtitleChange={(t) => view.onSubtitlePick(t)}
		activeAudioIndex={view.activeAudioIndex}
		onAudioPick={(id) => view.onAudioPick(id)}
		fullscreenTarget={wrapper}
		pip={view.pip}
		nextEpisode={view.nextEpisode}
		debugInfo={view.debugInfo}
		onVisibleChange={(v) => active && view.onControlsVisible(v)}
		onVolumeChange={(v, m) => view.onVolumeChange(v, m)}
		top={view.top}
		keptFor={view.keptFor}
		onsay={(t) => view.say(t)}
	/>
</div>

<style>
	/* always dark: the tokens inside resolve to their dark side, whatever the page's theme */
	.stage {
		color-scheme: dark;
		position: relative;
		width: 100%;
		height: 100%;
		min-height: 0;
		overflow: hidden;
		background: var(--stage);
		color: var(--stage-ink);
		border-radius: inherit;
	}
	/* the ring drawn inside: the page's frame clips anything outside the stage */
	.stage:focus-visible {
		outline: none;
		box-shadow: inset 0 0 0 3px var(--accent);
	}
	.stage.still {
		cursor: none;
	}
	.slot,
	.slot > :global(.video-host) {
		position: absolute;
		inset: 0;
	}
	/* the engines' own elements (they name the classes of the React app's utilities) */
	.slot :global(video),
	.slot :global(canvas) {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: contain;
		background: var(--stage);
	}
	.slot :global(video::cue) {
		background-color: var(--cue-bg);
		color: var(--cue-ink);
	}
	/* the ASS / PGS canvases, appended to the stage itself */
	.stage > :global(canvas) {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	.cue-guard {
		position: absolute;
		inset: 0;
		pointer-events: none;
		background: color-mix(in srgb, var(--stage) 1%, transparent);
	}
	.busy {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		pointer-events: none;
		color: var(--stage-muted);
	}
	.status {
		position: absolute;
		left: 50%;
		top: 50%;
		transform: translate(-50%, calc(-50% + var(--s-7)));
		margin: 0;
		padding: var(--s-1) var(--s-3);
		border-radius: var(--radius-pill);
		background: var(--stage-scrim);
		color: var(--stage-ink);
		font: var(--t-label);
		pointer-events: none;
		max-width: calc(100% - 2 * var(--s-4));
		text-align: center;
		z-index: 5;
	}
	/* kept in the page while silent: a live region must exist before it speaks */
	.status:empty {
		padding: 0;
		background: transparent;
	}
	.status.said {
		transform: translate(-50%, -50%);
	}
	:global(.pip-body) {
		background: var(--stage);
	}
</style>
