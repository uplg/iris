<script lang="ts">
	// Iris player shell. Picks the engine for the tier (Tier A static, the others imported on
	// demand), mounts it into a host element created once (so a move into a picture-in-picture
	// window carries the media element along instead of remounting it), and layers the stage:
	// overlay subtitles and the chrome. Generic: the watch page and live TV both use it.
	import { mount, unmount, untrack, type Snippet } from 'svelte';
	import { isWindowsChromium } from '@iris/core/caps';
	import type { EngineHandle } from '@iris/core/engine';
	import type { DecodeTier, Manifest, SubtitleTrack } from '@iris/core/manifest-client';
	import { attachMediaSession } from '@iris/core/os/media-session';
	import { ui } from '#lib/ui.svelte.ts';
	import { MediaState } from './media-state.svelte.ts';
	import { DocumentPip, isDocumentPipSupported } from './document-pip.svelte.ts';
	import { engineLoader } from './browser.ts';
	import { liftCues } from './cues.ts';
	import {
		classifySubtitles,
		initialAudioIndex,
		initialSubtitle,
		subtitleOverlayKind,
		tierRequiresRemountForAudio,
		versioned
	} from './tracks.ts';
	import PlayerStage, { type StageView } from './PlayerStage.svelte';
	import type { NextEpisode } from './IrisChrome.svelte';

	/** A decode error this soon after the tab comes back is a decoder the OS took away while
	 * hidden (Firefox/macOS releases VideoToolbox sessions), not a file it cannot play. */
	const TAB_RETURN_WINDOW_MS = 10_000;
	/** Healthy playback this long after that silent retry arms it again. */
	const RETRY_REARM_MS = 30_000;

	export interface Props {
		tier: DecodeTier;
		src: string;
		/** Live TV: endless, joined at the live edge, no timeline. */
		live?: boolean;
		title: string;
		manifest: Manifest;
		startPosition: number;
		/** The per-file picks restored from progress (`null` subtitle: turned off on purpose). */
		initialAudioIndex?: number;
		initialSubtitleStreamIdx?: number | null;
		/** Preferred languages, applied when the file has no pick of its own. */
		preferredAudioLang?: string | null;
		preferredSubtitleLang?: string | null;
		initialVolume?: number;
		onVolumeChange?: (volume: number, muted: boolean) => void;
		nextEpisode?: NextEpisode | null;
		/** Bumped as the torrent downloads (`final` once finished): subtitle URLs carry it as `?v=`
		 * so the extractions are fetched again, in place. */
		subtitleVersion?: string;
		/** The top bar of the chrome. */
		top?: Snippet;
		keptFor?: string;
		/** A problem said on the stage (the page's: server unreachable, player error). */
		notice?: string | null;
		onTimeUpdate?: (seconds: number) => void;
		onDurationChange?: (seconds: number) => void;
		onSeeking?: (seconds: number) => void;
		onPause?: (seconds: number) => void;
		onEnded?: () => void;
		onError: (message: string) => void;
		onAudioTrackChange?: (index: number) => void;
		onActiveSubtitleChange?: (streamIdx: number | null) => void;
		/** Test seam: the engine to mount instead of the tier's. */
		mountOverride?: import('@iris/core/engine').EngineMount;
	}
	let props: Props = $props();

	const live = $derived(props.live === true);
	const media = new MediaState(untrack(() => props.manifest.duration_s ?? null));
	const pip = new DocumentPip(720, 405);
	const pipSupported = isDocumentPipSupported();

	// created once, outside the stage's markup: the stage re-attaches it wherever it is drawn
	const videoHost = document.createElement('div');
	videoHost.className = 'video-host';

	// raw: the handle is a plain object of methods, not state to deep-proxy
	let handle = $state.raw<EngineHandle | null>(null);
	/** Pushes the play state and position to the OS media session (set while one is wired). */
	let syncSession = () => undefined as void;
	let controlsVisible = $state(true);
	let volume: number | null = untrack(() => props.initialVolume ?? null);
	let currentTime = untrack(() => props.startPosition);
	let playingBeforeRemount = false;
	let mountStart = $state(untrack(() => props.startPosition));
	let remountVersion = $state(0);

	let audioIndex = $state(untrack(() => initialAudioIndex(props.manifest, props.initialAudioIndex, props.preferredAudioLang)));
	let activeSubtitle = $state<SubtitleTrack | null>(
		untrack(() => initialSubtitle(props.manifest, props.initialSubtitleStreamIdx, props.preferredSubtitleLang))
	);

	const subs = $derived(classifySubtitles(props.manifest));

	// background-killed decoders: the signature is a decode error just after a tab return
	let lastVisibleReturn = Number.NEGATIVE_INFINITY;
	let decodeRetryAt: number | null = null;
	$effect(() => {
		const onVisibility = () => {
			if (!document.hidden) lastVisibleReturn = performance.now();
		};
		document.addEventListener('visibilitychange', onVisibility);
		return () => document.removeEventListener('visibilitychange', onVisibility);
	});

	function mountEngine(tier: DecodeTier, src: string): () => void {
		media.busy = true;
		const loader = props.mountOverride ? () => Promise.resolve({ mount: props.mountOverride! }) : engineLoader(tier, live);
		if (!loader) {
			props.onError(`No engine wired for tier ${tier}`);
			return () => undefined;
		}
		let cancelled = false;
		let mounted: EngineHandle | null = null;
		let unfollow = () => undefined as void;
		// one report per failure: engines both call `onError` and throw while mounting
		let reported = false;
		const report = (message: string) => {
			if (cancelled || reported) return;
			reported = true;
			props.onError(message);
		};
		void (async () => {
			try {
				const { mount: mountFn } = await loader();
				if (cancelled) return;
				const h = await mountFn({
					container: videoHost,
					manifest: props.manifest,
					streamUrl: src,
					// the live playhead, not the page's resume point: a remount for a new src (an
					// outage recovered, a demotion) must not rewind, nor heartbeats clobber the real
					// position; live joins the edge
					startPosition: live ? 0 : currentTime > 0 ? currentTime : mountStart,
					nativeSubs: subs.native,
					audioTrackIndex: audioIndex,
					live,
					// a mount being torn down (a remount, a demotion) speaks no more: its late error
					// would be charged to the next tier, its late end would mark the file watched
					onTimeUpdate: (t) => {
						if (cancelled) return;
						// a final 0 while tearing down would poison the resume position
						if (t > 0) currentTime = t;
						if (decodeRetryAt !== null && performance.now() - decodeRetryAt > RETRY_REARM_MS) decodeRetryAt = null;
						media.read(mounted);
						media.time = t;
						props.onTimeUpdate?.(t);
					},
					onBusyChange: (b) => {
						if (!cancelled) media.busy = b;
					},
					// canvas tiers have no element for `onBusyChange`: ready clears the spinner
					onReady: () => {
						if (!cancelled) media.busy = false;
					},
					onDurationChange: (d) => {
						if (cancelled) return;
						if (d > 0) media.duration = d;
						syncSession();
						props.onDurationChange?.(d);
					},
					onPlayingChange: () => {
						if (cancelled) return;
						media.read(mounted);
						syncSession();
					},
					onSeeking: (t) => {
						if (cancelled) return;
						syncSession();
						props.onSeeking?.(t);
					},
					onPause: (t) => {
						if (cancelled) return;
						media.read(mounted);
						syncSession();
						props.onPause?.(t);
					},
					onEnded: () => {
						if (!cancelled) props.onEnded?.();
					},
					onError: (err) => {
						if (cancelled) return;
						const sinceReturn = performance.now() - lastVisibleReturn;
						if (/media error 3\b/.test(err.message) && sinceReturn < TAB_RETURN_WINDOW_MS && decodeRetryAt === null) {
							decodeRetryAt = performance.now();
							console.warn(
								`[iris-core] decode error ${(sinceReturn / 1000).toFixed(1)}s after tab return — decoder likely released while hidden; remounting same tier at ${currentTime.toFixed(1)}s`
							);
							playingBeforeRemount = true;
							mountStart = currentTime;
							remountVersion += 1;
							return;
						}
						report(err.message);
					}
				});
				if (cancelled) {
					void h.dispose();
					return;
				}
				mounted = h;
				handle = h;
				// a fresh <video> starts at full volume: the device's level again
				if (volume !== null) h.setVolume(Math.max(0, Math.min(1, volume)));
				unfollow = media.follow(h);
				// was playing before this remount (an audio switch, the silent decoder retry)
				if (playingBeforeRemount) {
					playingBeforeRemount = false;
					void h.play().catch(() => undefined);
				}
			} catch (e) {
				report(e instanceof Error ? e.message : String(e));
			}
		})();
		return () => {
			cancelled = true;
			unfollow();
			if (mounted) {
				// OR, never overwrite: the decode retry sets it before remounting, and an errored
				// element may misreport `paused`
				try {
					playingBeforeRemount = playingBeforeRemount || !mounted.paused();
				} catch {
					// keep what was set
				}
			}
			handle = null;
			void mounted?.dispose();
		};
	}

	// remount on tier, src, the start position and the explicit counter only: the audio pick
	// travels by closure (a Tier F switch must not remount and snap back to hls.js' default)
	$effect(() => {
		const tier = props.tier;
		const src = props.src;
		void mountStart;
		void remountVersion;
		return untrack(() => mountEngine(tier, src));
	});

	// native subtitles re-fetched as the torrent downloads (declared before the mode flip so the
	// versioned src is in place when the track first shows)
	$effect(() => {
		const h = handle;
		if (!h?.setNativeSubtitleSrc) return;
		for (const s of subs.native) h.setNativeSubtitleSrc(s.stream_idx, versioned(s.vttUrl, props.subtitleVersion));
	});

	$effect(() => {
		const h = handle;
		if (!h) return;
		const sub = activeSubtitle;
		const kind = sub ? subtitleOverlayKind(sub) : 'none';
		h.setNativeSubtitle(kind === 'native' ? (sub?.stream_idx ?? null) : null);
		if (sub) console.log(`[iris-core] active subtitle: stream=${sub.stream_idx} kind=${kind} codec=${sub.codec} url=${sub.url}`);
	});

	// native cues lifted above the chrome while it shows
	$effect(() => {
		const v = handle?.videoElement();
		if (!v || subtitleOverlayKind(activeSubtitle) !== 'native') return;
		return liftCues(v, controlsVisible);
	});

	// OS media keys and lock screen, for every tier (canvas ones included)
	$effect(() => {
		const h = handle;
		if (!h) return;
		const wire = attachMediaSession(h, props.manifest, { title: props.title });
		syncSession = wire.sync;
		return () => {
			syncSession = () => undefined;
			wire.dispose();
		};
	});

	const activeOverlay = $derived.by(() => {
		const sub = activeSubtitle;
		if (!sub || subtitleOverlayKind(sub) === 'native') return null;
		const base = subs.overlay.find((s) => s.stream_idx === sub.stream_idx);
		return base ? { ...base, url: versioned(base.url, props.subtitleVersion) } : null;
	});

	function onSubtitlePick(track: SubtitleTrack | null) {
		activeSubtitle = track;
		props.onActiveSubtitleChange?.(track ? track.stream_idx : null);
	}

	function onAudioPick(id: string) {
		const idx = Number(id);
		if (!Number.isFinite(idx)) return;
		console.log(
			`[iris-core] onAudioPick id=${id} tier=${props.tier} hasHandle=${!!handle} needsRemount=${tierRequiresRemountForAudio(props.tier)}`
		);
		audioIndex = idx;
		props.onAudioTrackChange?.(idx);
		if (tierRequiresRemountForAudio(props.tier)) {
			// the playhead before the teardown (the disposed engine reads 0)
			mountStart = currentTime;
			remountVersion += 1;
		} else if (handle) {
			handle.setAudioTrack(id);
		} else {
			console.warn("[iris-core] onAudioPick: tier doesn't need remount but handle is null — swallowed");
		}
	}

	const debugInfo = $derived.by<[string, string][]>(() => {
		const v = props.manifest.video[0];
		const a = props.manifest.audio[audioIndex] ?? props.manifest.audio[0];
		return [
			['tier', `${props.tier}${live ? ' (live)' : ''}`],
			['source', props.manifest.container],
			['video', v ? `${v.codec} ${v.codec_string ?? ''} ${v.width ?? '?'}x${v.height ?? '?'}`.trim() : 'none'],
			['audio', a ? `${a.codec} ${a.codec_string ?? ''} ${a.channels ?? '?'}ch ${a.lang ?? '?'}`.trim() : 'none'],
			['subs', `${props.manifest.subtitles.length} track(s), active ${activeSubtitle?.codec ?? 'none'}`],
			['agent', navigator.userAgent]
		];
	});

	const view: StageView = {
		videoHost,
		get handle() {
			return handle;
		},
		media,
		get manifest() {
			return props.manifest;
		},
		get live() {
			return live;
		},
		get title() {
			return props.title;
		},
		get activeSubtitle() {
			return activeSubtitle;
		},
		get activeOverlay() {
			return activeOverlay;
		},
		get activeAudioIndex() {
			return audioIndex;
		},
		get cueGuard() {
			return !live && subtitleOverlayKind(activeSubtitle) === 'native' && isWindowsChromium();
		},
		pip: {
			supported: pipSupported,
			get active() {
				return pip.active;
			},
			toggle: () => pip.toggle()
		},
		get nextEpisode() {
			return props.nextEpisode ?? null;
		},
		get debugInfo() {
			return debugInfo;
		},
		get top() {
			return props.top;
		},
		get keptFor() {
			return props.keptFor;
		},
		get notice() {
			return props.notice ?? null;
		},
		get controlsVisible() {
			return controlsVisible;
		},
		get pipOpen() {
			return pip.active;
		},
		// the engine's live clock (the overlays read it every frame), the last reported time
		// between engines
		getCurrentTime: () => {
			try {
				const t = handle?.currentTime();
				if (t !== undefined && Number.isFinite(t)) return t;
			} catch {
				// an engine mid-teardown
			}
			return currentTime;
		},
		onSubtitlePick,
		onAudioPick,
		onVolumeChange: (v, m) => {
			volume = v;
			props.onVolumeChange?.(v, m);
		},
		onControlsVisible: (v) => (controlsVisible = v),
		say: (t) => ui.say(t)
	};

	// in picture in picture, a stage of its own lives in that window (its events are delegated
	// there); the page's stays, covered by a placeholder, to take the picture back on close
	$effect(() => {
		const win = pip.window;
		if (!win) return;
		const stage = mount(PlayerStage, { target: win.document.body, props: { view, place: 'pip' } });
		return () => {
			void unmount(stage);
		};
	});

	// the player going away (navigation, the next episode) takes its window along
	$effect(() => () => pip.close());
</script>

<div class="player">
	<PlayerStage {view} />
	{#if pip.window}
		<div class="away">
			<p>Playing in picture in picture.</p>
			<button class="btn" type="button" onclick={() => void pip.toggle()}>Bring it back here</button>
		</div>
	{/if}
</div>

<style>
	.player {
		position: relative;
		width: 100%;
		height: 100%;
		border-radius: inherit;
	}
	.away {
		color-scheme: dark;
		position: absolute;
		inset: 0;
		z-index: 20;
		display: grid;
		place-content: center;
		justify-items: center;
		gap: var(--s-3);
		background: var(--stage);
		color: var(--stage-ink);
		text-align: center;
		padding: var(--s-4);
	}
	.away p {
		margin: 0;
	}
	.away .btn {
		min-height: var(--control-h);
	}
</style>
