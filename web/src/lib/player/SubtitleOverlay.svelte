<script lang="ts">
	// ASS (libass) and bitmap (libpgs) subtitles drawn on a canvas over the picture; text subs go
	// through the engine's own <track> and draw nothing here. Two lifetimes: the renderer lives as
	// long as the track's identity (stream, codec) and host, and a URL change alone (the `?v=`
	// bump as the torrent downloads) re-fetches in place: no worker recreated, no blank flash,
	// no new pick from the menu.
	import type { SubtitleTrack } from '@iris/core/manifest-client';
	import { mountAssOverlay, type AssOverlayHandle } from '@iris/core/subs/ass-overlay';
	import { mountPgsOverlay, type PgsOverlayHandle } from '@iris/core/subs/pgs-overlay';
	import { subtitleOverlayKind } from './tracks.ts';

	interface Props {
		host: HTMLElement | null;
		track: SubtitleTrack | null;
		/** The clock the renderer follows (no <video> handed over: it reads this every frame). */
		getCurrentTime: () => number;
	}
	let { host, track, getCurrentTime }: Props = $props();

	const kind = $derived(subtitleOverlayKind(track));
	const streamIdx = $derived(track?.stream_idx ?? null);
	const codec = $derived(track?.codec ?? null);
	const url = $derived(track?.url ?? null);

	let overlay: AssOverlayHandle | PgsOverlayHandle | null = null;
	// read by the mount's late `.then`: the URL may have moved on while the worker spun up
	let latestUrl: string | null = null;
	$effect(() => {
		latestUrl = url;
	});

	$effect(() => {
		const target = host;
		const k = kind;
		void streamIdx;
		void codec;
		if (!target || (k !== 'ass' && k !== 'pgs')) return;
		const initial = latestUrl;
		if (!initial) return;
		let cancelled = false;
		const mountFn = k === 'ass' ? mountAssOverlay : mountPgsOverlay;
		void (async () => {
			try {
				const h = await mountFn({ host: target, subUrl: initial, getCurrentTime: () => getCurrentTime() });
				if (cancelled) return h.dispose();
				overlay = h;
				if (latestUrl && latestUrl !== initial) h.setUrl(latestUrl);
			} catch (e) {
				console.error(`[iris-core] ${k} overlay failed`, e);
			}
		})();
		return () => {
			cancelled = true;
			overlay?.dispose();
			overlay = null;
		};
	});

	let lastUrl: string | null = null;
	$effect(() => {
		const next = url;
		if (next && lastUrl && next !== lastUrl) overlay?.setUrl(next);
		lastUrl = next;
	});
</script>
