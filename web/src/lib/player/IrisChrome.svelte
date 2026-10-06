<script lang="ts" module>
	export type ChromePip = { supported: boolean; active: boolean; toggle: () => Promise<void> };
	export type NextEpisode = { label: string; onPlay: () => void };
	/** How long the controls stay after the last movement while playing. */
	export const HIDE_AFTER_MS = 2500;
</script>

<script lang="ts">
	// The player's controls over the stage: a top bar (the page's own snippet), the timeline,
	// one row of 44 px buttons, the audio/subtitles panel and the shortcuts list. Engine-agnostic:
	// it only sees the EngineHandle and the MediaState its events keep current.
	//
	// The controls hide after a still moment while playing, never while paused, scrubbing, a panel
	// is open, or a control holds the keyboard focus (WCAG 2.4.7, 2.2.1): a key or a movement
	// brings them back, and so does the focus arriving on one of them.
	import type { Snippet } from 'svelte';
	import { untrack } from 'svelte';
	import type { EngineHandle } from '@iris/core/engine';
	import type { Manifest, SubtitleTrack } from '@iris/core/manifest-client';
	import { clock } from '@iris/api/format';
	import Icon, { type IconName } from '#lib/components/Icon.svelte';
	import Range from '#lib/components/Range.svelte';
	import type { MediaState } from './media-state.svelte.ts';
	import ScrubBar from './ScrubBar.svelte';
	import TrackPanel from './TrackPanel.svelte';
	import { actionFor, nextSpeed, ownedByControl, shortcutList, type PlayerAction } from './shortcuts.ts';
	import { volumeText } from './spoken.ts';
	import { toggleFullscreen } from './browser.ts';

	interface Props {
		handle: EngineHandle | null;
		media: MediaState;
		manifest: Manifest;
		/** An endless stream: no timeline, no skips, a « Live » label. */
		live?: boolean;
		activeSubtitle: SubtitleTrack | null;
		onSubtitleChange: (track: SubtitleTrack | null) => void;
		/** Index into `manifest.audio`. */
		activeAudioIndex: number;
		onAudioPick: (id: string) => void;
		/** The stage: full screen target, where the keys are listened to. */
		fullscreenTarget: HTMLElement | null;
		pip: ChromePip;
		nextEpisode?: NextEpisode | null;
		/** Facts for the playback details, before what the engine reports. */
		debugInfo?: [string, string][];
		onVisibleChange?: (visible: boolean) => void;
		onVolumeChange?: (volume: number, muted: boolean) => void;
		/** The top bar's content (the way back, the title, the facts line). */
		top?: Snippet;
		/** Where an audio/subtitle choice is kept, said in the panel. */
		keptFor?: string;
		/** Said politely to screen readers (a speed change). */
		onsay?: (text: string) => void;
	}
	let {
		handle,
		media,
		manifest,
		live = false,
		activeSubtitle,
		onSubtitleChange,
		activeAudioIndex,
		onAudioPick,
		fullscreenTarget,
		pip,
		nextEpisode = null,
		debugInfo = [],
		onVisibleChange,
		onVolumeChange,
		top,
		keptFor,
		onsay
	}: Props = $props();

	type Panel = 'none' | 'tracks' | 'shortcuts';
	let panel = $state<Panel>('none');
	let details = $state(false);
	let scrubTo = $state<number | null>(null);
	let recent = $state(true);
	let controlFocused = $state(false);
	let fullscreen = $state(false);
	let tracksButton = $state<HTMLButtonElement | null>(null);
	let shortcutsButton = $state<HTMLButtonElement | null>(null);

	const shownTime = $derived(scrubTo ?? media.time);
	const remaining = $derived(media.duration !== null ? Math.max(0, media.duration - shownTime) : null);
	const blocked = $derived(media.paused || panel !== 'none' || scrubTo !== null || controlFocused);
	const visible = $derived(recent || blocked);
	const hasTracks = $derived(manifest.subtitles.length > 0 || manifest.audio.length > 1);
	const canSpeed = $derived(!live && !!handle?.videoElement());

	$effect(() => onVisibleChange?.(visible));

	// approved web timer: the controls' auto-hide after HIDE_AFTER_MS of stillness while playing
	// (CLAUDE.md rule 2; never while `blocked`: see `visible`)
	let hideTimer: ReturnType<typeof setTimeout> | undefined;
	function wake() {
		recent = true;
		clearTimeout(hideTimer);
		hideTimer = setTimeout(() => (recent = false), HIDE_AFTER_MS);
	}
	$effect(() => () => clearTimeout(hideTimer));
	// a blocker lifted (a panel closed, playback resumed): a fresh still moment before hiding
	$effect(() => {
		if (!blocked) untrack(wake);
	});

	function refresh() {
		media.read(handle);
	}

	function togglePlay() {
		if (!handle) return;
		if (handle.paused()) void handle.play().catch(() => undefined);
		else handle.pause();
		refresh();
	}
	function seekTo(s: number) {
		if (!handle) return;
		handle.seek(Math.max(0, s));
		refresh();
	}
	function seekBy(d: number) {
		if (!handle || live) return;
		seekTo(handle.currentTime() + d);
	}
	function setVolume(v: number) {
		if (!handle) return;
		const next = Math.max(0, Math.min(1, v));
		handle.setVolume(next);
		if (next > 0 && handle.muted()) handle.setMuted(false);
		refresh();
		onVolumeChange?.(handle.volume(), handle.muted());
	}
	function toggleMute() {
		if (!handle) return;
		handle.setMuted(!handle.muted());
		refresh();
		onVolumeChange?.(handle.volume(), handle.muted());
	}
	function changeSpeed(step: 1 | -1) {
		const v = handle?.videoElement();
		if (!v || live) return;
		v.playbackRate = nextSpeed(v.playbackRate, step);
		refresh();
		onsay?.(v.playbackRate === 1 ? 'Normal speed' : `Speed ${v.playbackRate}×`);
	}
	function fs() {
		void toggleFullscreen(fullscreenTarget);
	}
	function openPanel(p: Panel) {
		panel = panel === p ? 'none' : p;
		if (panel === 'none') (p === 'tracks' ? tracksButton : shortcutsButton)?.focus();
	}
	function closePanel() {
		const was = panel;
		panel = 'none';
		(was === 'tracks' ? tracksButton : shortcutsButton)?.focus();
	}

	function run(a: PlayerAction) {
		switch (a.kind) {
			case 'toggle-play':
				return togglePlay();
			case 'seek-by':
				return seekBy(a.seconds);
			case 'seek-to-fraction':
				if (handle && media.duration) seekTo(media.duration * a.fraction);
				return;
			case 'volume-by':
				return handle && setVolume(handle.volume() + a.delta);
			case 'toggle-mute':
				return toggleMute();
			case 'fullscreen':
				return fs();
			case 'tracks':
				return hasTracks && openPanel('tracks');
			case 'pip':
				return pip.supported && void pip.toggle();
			case 'shortcuts':
				return openPanel('shortcuts');
			case 'speed-by':
				return changeSpeed(a.step);
			case 'escape':
				if (panel !== 'none') closePanel();
				return;
		}
	}

	// the keys, on the stage (so they act wherever the focus is inside the player)
	$effect(() => {
		const el = fullscreenTarget;
		if (!el || !handle) return;
		const onKey = (e: KeyboardEvent) => {
			wake();
			if (ownedByControl(e.target, e.key)) return;
			const a = actionFor(e, live);
			if (!a) return;
			// Escape with nothing open is the browser's (it leaves full screen)
			if (a.kind === 'escape' && panel === 'none') return;
			e.preventDefault();
			run(a);
		};
		if (el.tabIndex < 0) el.tabIndex = 0;
		el.addEventListener('keydown', onKey);
		return () => el.removeEventListener('keydown', onKey);
	});

	// the pointer, on the whole stage (the controls layer lets it through to the picture)
	$effect(() => {
		const el = fullscreenTarget;
		if (!el) return;
		const onDown = (e: PointerEvent) => {
			outside(e);
			// a touch has no hover: a tap on hidden controls only brings them back (the stage's
			// click that follows does not also toggle playback)
			if (e.pointerType === 'touch' && !visible) el.dataset.pressConsumed = '';
			wake();
		};
		// a lifted finger leaves too (touch pointers end in pointerleave): only a mouse leaving
		// the stage hides the controls at once
		const onLeave = (e: PointerEvent) => {
			if (media.paused || e.pointerType === 'touch') return;
			clearTimeout(hideTimer);
			recent = false;
		};
		el.addEventListener('pointermove', wake);
		el.addEventListener('pointerdown', onDown);
		el.addEventListener('pointerleave', onLeave);
		return () => {
			el.removeEventListener('pointermove', wake);
			el.removeEventListener('pointerdown', onDown);
			el.removeEventListener('pointerleave', onLeave);
		};
	});

	// full screen state, for the button's words
	$effect(() => {
		const el = fullscreenTarget;
		if (!el) return;
		const doc = el.ownerDocument;
		const on = () => (fullscreen = doc.fullscreenElement === el);
		on();
		doc.addEventListener('fullscreenchange', on);
		return () => doc.removeEventListener('fullscreenchange', on);
	});

	// a pointer press outside an open panel closes it, and only that: the stage's click that
	// follows does not also toggle playback (`data-press-consumed`, read by PlayerStage)
	function outside(e: PointerEvent) {
		const el = fullscreenTarget;
		if (el) delete el.dataset.pressConsumed;
		if (panel === 'none') return;
		const t = e.target as Element | null;
		if (t?.closest('.stage-panel, [data-panel-trigger]')) return;
		panel = 'none';
		if (el) el.dataset.pressConsumed = '';
	}

	const stats = $derived.by(() => {
		if (!details || !handle) return [] as [string, string][];
		// refreshed with the playhead's events while open
		void media.time;
		void media.paused;
		void media.buffered;
		return handle.stats?.() ?? [];
	});

	interface Btn {
		label: string;
		key: string;
		icon: IconName;
		onclick: () => void;
		pressed?: boolean;
	}
	const playBtn = $derived<Btn>({
		label: media.paused ? 'Play' : 'Pause',
		key: 'Space',
		icon: media.paused ? 'play' : 'pause',
		onclick: togglePlay
	});
</script>

{#snippet button(b: Btn, extra?: { ref?: (el: HTMLButtonElement) => void; expanded?: boolean; trigger?: boolean; wide?: boolean })}
	<button
		type="button"
		class={['ctl', extra?.wide && 'wide-only']}
		aria-label="{b.label}, {b.key}"
		aria-keyshortcuts={b.key === 'Space' ? 'Space k' : b.key}
		aria-pressed={b.pressed}
		aria-expanded={extra?.expanded}
		aria-haspopup={extra?.expanded === undefined ? undefined : 'dialog'}
		data-panel-trigger={extra?.trigger ? '' : undefined}
		{@attach (el: HTMLButtonElement) => extra?.ref?.(el)}
		onclick={b.onclick}
	>
		<Icon name={b.icon} size={22} />
		<span class="tip" aria-hidden="true">{b.label} <kbd>{b.key}</kbd></span>
	</button>
{/snippet}

{#if handle}
	<div
		class="chrome"
		class:shown={visible}
		role="presentation"
		onfocusin={(e) => {
			wake();
			const t = e.target as Element;
			controlFocused = t.matches(':focus-visible') && !!t.closest('button, a[href], input, [role="slider"]');
		}}
		onfocusout={(e) => {
			if (!(e.relatedTarget instanceof Element) || !e.currentTarget.contains(e.relatedTarget)) controlFocused = false;
		}}
	>
		<div class="top" data-iris-chrome>
			{@render top?.()}
		</div>

		<div class="middle">
			{#if details}
				<dl class="details" data-iris-chrome aria-label="Playback details">
					{#each [...debugInfo, ...stats] as [k, v], i (i)}
						<div>
							<dt>{k}</dt>
							<dd>{v}</dd>
						</div>
					{/each}
				</dl>
			{/if}
			{#if panel === 'tracks'}
				<TrackPanel
					audio={manifest.audio}
					activeAudio={activeAudioIndex}
					subtitles={manifest.subtitles}
					activeSubtitle={activeSubtitle?.stream_idx ?? null}
					onaudio={(i) => onAudioPick(String(i))}
					onsubtitle={onSubtitleChange}
					{keptFor}
					onclose={closePanel}
				/>
			{:else if panel === 'shortcuts'}
				<div
					class="stage-panel"
					role="dialog"
					aria-labelledby="shortcuts-title"
					tabindex="-1"
					data-iris-chrome
					{@attach (el) => el.focus()}
					onkeydown={(e) => {
						if (e.key === 'Escape') {
							e.stopPropagation();
							closePanel();
						}
					}}
				>
					<div class="panel-head">
						<h2 id="shortcuts-title" class="panel-title">Keyboard shortcuts</h2>
						<button class="ctl" type="button" aria-label="Close" onclick={closePanel}><Icon name="x" /></button>
					</div>
					<dl class="keys">
						{#each shortcutList(live) as s (s.does)}
							<div>
								<dt>
									{#each s.keys as k (k)}<kbd>{k}</kbd>{/each}
								</dt>
								<dd>{s.does}</dd>
							</div>
						{/each}
					</dl>
					<button class="text-btn" type="button" aria-pressed={details} onclick={() => (details = !details)}>
						{details ? 'Hide playback details' : 'Show playback details'}
					</button>
				</div>
			{/if}
		</div>

		<div class="bottom" data-iris-chrome>
			{#if live}
				<div class="row live-row">
					<span class="live"><span class="dot" aria-hidden="true"></span>Live</span>
				</div>
			{:else}
				<div class="row scrub-row">
					<span class="time" aria-hidden="true">{clock(shownTime)}</span>
					<ScrubBar
						duration={media.duration}
						current={shownTime}
						buffered={media.buffered}
						chapters={manifest.chapters}
						onscrub={(s) => (scrubTo = s)}
						onseek={(s) => {
							scrubTo = null;
							seekTo(s);
						}}
						oncancel={() => (scrubTo = null)}
					/>
					<span class="time" aria-hidden="true">{remaining !== null ? `−${clock(remaining)}` : '--:--'}</span>
				</div>
			{/if}
			<div class="row controls">
				{@render button(playBtn)}
				{#if !live}
					{@render button({ label: 'Back 10 seconds', key: 'J', icon: 'rotate-ccw', onclick: () => seekBy(-10) })}
					{@render button({ label: 'Forward 10 seconds', key: 'L', icon: 'rotate-cw', onclick: () => seekBy(10) })}
				{/if}
				<div class="volume">
					{@render button({
						label: media.muted ? 'Unmute' : 'Mute',
						key: 'M',
						icon: media.muted || media.volume === 0 ? 'volume-x' : 'volume-2',
						onclick: toggleMute
					})}
					<div class="volume-range">
						<Range
							label="Volume"
							hideLabel
							value={media.muted ? 0 : Math.round(media.volume * 100)}
							step={5}
							valueText={(v) => volumeText(v / 100, media.muted && v === 0)}
							live
							send={async (v) => setVolume(v / 100)}
						/>
					</div>
				</div>
				<span class="spacer"></span>
				{#if canSpeed && media.rate !== 1}<span class="fact">{media.rate}×</span>{/if}
				{#if nextEpisode && !live}
					<button type="button" class="text-btn next" aria-label={nextEpisode.label} onclick={nextEpisode.onPlay}>
						<Icon name="skip-forward" /><span class="next-label">{nextEpisode.label}</span>
					</button>
				{/if}
				{#if hasTracks}
					{@render button(
						{ label: 'Audio and subtitles', key: 'C', icon: 'captions', onclick: () => openPanel('tracks'), pressed: undefined },
						{ ref: (el) => (tracksButton = el), expanded: panel === 'tracks', trigger: true }
					)}
				{/if}
				{#if pip.supported}
					{@render button(
						{
							label: pip.active ? 'Leave picture in picture' : 'Picture in picture',
							key: 'P',
							icon: 'picture-in-picture',
							onclick: () => void pip.toggle(),
							pressed: pip.active
						},
						{ wide: !pip.active }
					)}
				{/if}
				{@render button({
					label: fullscreen ? 'Leave full screen' : 'Full screen',
					key: 'F',
					icon: fullscreen ? 'minimize' : 'maximize',
					onclick: fs
				})}
				{@render button(
					{ label: 'Keyboard shortcuts', key: '?', icon: 'info', onclick: () => openPanel('shortcuts') },
					{ ref: (el) => (shortcutsButton = el), expanded: panel === 'shortcuts', trigger: true, wide: true }
				)}
			</div>
		</div>
	</div>
{/if}

<style>
	/* the controls layer: clicks on its empty middle reach the stage (play/pause, full screen) */
	.chrome {
		position: absolute;
		inset: 0;
		z-index: 10;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		color: var(--stage-ink);
		pointer-events: none;
	}
	.top,
	.bottom {
		pointer-events: auto;
		background: var(--stage-scrim);
		opacity: 0;
		transition: opacity 0.2s ease;
	}
	.chrome.shown .top,
	.chrome.shown .bottom {
		opacity: 1;
	}
	/* hidden, a control is still reached by Tab: arriving there shows the bar (onfocusin) */
	.chrome:not(.shown) .top,
	.chrome:not(.shown) .bottom {
		pointer-events: none;
	}
	.top {
		padding: var(--s-2) var(--s-3);
	}
	.top:empty {
		display: none;
	}
	.middle {
		position: relative;
		flex: 1;
		min-height: 0;
		display: flex;
		align-items: flex-end;
		justify-content: flex-end;
		padding: var(--s-2);
		gap: var(--s-2);
	}
	.bottom {
		display: grid;
		gap: var(--s-1);
		padding: var(--s-1) var(--s-3) var(--s-2);
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--s-1);
		min-width: 0;
	}
	.scrub-row {
		gap: var(--s-3);
	}
	.time {
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		color: var(--stage-ink);
		flex: none;
	}
	.controls {
		flex-wrap: wrap;
	}
	.spacer {
		flex: 1;
	}
	.ctl {
		position: relative;
		display: grid;
		place-items: center;
		width: var(--control-h);
		height: var(--control-h);
		border: 0;
		border-radius: var(--radius);
		background: transparent;
		color: var(--stage-ink);
		cursor: pointer;
		flex: none;
	}
	.ctl:hover,
	.ctl[aria-pressed='true'],
	.ctl[aria-expanded='true'] {
		background: var(--stage-line);
	}
	.tip {
		position: absolute;
		bottom: calc(100% + var(--s-1));
		left: 50%;
		transform: translateX(-50%);
		display: none;
		gap: var(--s-1);
		align-items: center;
		padding: var(--s-1) var(--s-2);
		border-radius: var(--radius);
		background: var(--stage-raised);
		color: var(--stage-ink);
		font: var(--t-meta);
		white-space: nowrap;
		pointer-events: none;
	}
	.ctl:hover .tip,
	.ctl:focus-visible .tip {
		display: inline-flex;
	}
	.tip kbd,
	.keys kbd {
		background: var(--stage);
		color: var(--stage-muted);
		border-color: var(--stage-line);
	}
	.volume {
		display: flex;
		align-items: center;
	}
	.volume-range {
		width: 6.5rem;
	}
	/* a phone: the volume is the device's keys, the next episode an icon (its name stays its
	   label), the shortcuts list (no keyboard) and picture in picture leave the row; the panels become a sheet over the
	   page, the stage being too short to hold them */
	@media (max-width: 599px) {
		.volume-range,
		.wide-only,
		.next-label {
			display: none;
		}
		.text-btn.next {
			padding: 0;
			width: var(--control-h);
			justify-content: center;
		}
		.bottom {
			padding-inline: var(--s-1);
		}
		.row {
			gap: 0;
		}
		.scrub-row {
			gap: var(--s-2);
		}
		:global(.stage-panel) {
			position: fixed;
			z-index: 40;
			left: 0;
			right: 0;
			bottom: 0;
			width: 100%;
			max-height: 75dvh;
			border-radius: var(--radius-2xl) var(--radius-2xl) 0 0;
			padding-bottom: calc(var(--s-4) + env(safe-area-inset-bottom, 0px));
		}
	}
	.text-btn {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		min-height: var(--control-h);
		padding: 0 var(--s-3);
		border: 1px solid var(--stage-line);
		border-radius: var(--radius);
		background: transparent;
		color: var(--stage-ink);
		font: var(--t-label);
		cursor: pointer;
		max-width: 100%;
	}
	.text-btn:hover {
		background: var(--stage-line);
	}
	.fact {
		font: var(--t-meta);
		color: var(--stage-muted);
		padding-inline: var(--s-2);
	}
	.live-row {
		min-height: var(--control-h-xs);
	}
	.live {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		font: var(--t-label);
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}
	.dot {
		width: var(--s-2);
		height: var(--s-2);
		border-radius: 50%;
		background: var(--status-down);
	}
	/* panels inside the stage (not portalled: they stay in full screen) */
	:global(.stage-panel) {
		pointer-events: auto;
		display: flex;
		flex-direction: column;
		gap: var(--s-3);
		max-height: 100%;
		width: min(26rem, calc(100% - 2 * var(--s-3)));
		padding: var(--s-4);
		border: 1px solid var(--stage-line);
		border-radius: var(--radius-l);
		background: var(--stage-raised);
		color: var(--stage-ink);
		box-shadow: var(--shadow-pop);
		overflow: auto;
	}
	.panel-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.panel-title {
		font: var(--t-group);
	}
	.keys {
		margin: 0;
		display: grid;
		gap: var(--s-2);
	}
	.keys div {
		display: flex;
		gap: var(--s-3);
		align-items: baseline;
	}
	.keys dt {
		display: flex;
		gap: var(--s-1);
		min-width: 6rem;
	}
	.keys dd {
		margin: 0;
		font: var(--t-secondary);
	}
	.details {
		pointer-events: auto;
		position: absolute;
		top: var(--s-2);
		left: var(--s-2);
		max-width: min(28rem, calc(100% - 2 * var(--s-2)));
		max-height: 70%;
		overflow: auto;
		margin: 0;
		padding: var(--s-3);
		border-radius: var(--radius-l);
		background: var(--stage-scrim);
		font: var(--t-tiny);
		font-family: var(--font-mono);
	}
	.details div {
		display: flex;
		gap: var(--s-2);
	}
	.details dt {
		width: 5rem;
		flex: none;
		color: var(--stage-muted);
	}
	.details dd {
		margin: 0;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	@media (prefers-reduced-motion: reduce) {
		.top,
		.bottom {
			transition: none;
		}
	}
</style>
