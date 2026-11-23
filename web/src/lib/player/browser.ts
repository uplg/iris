// Small browser helpers the player shell shares: full screen, the device-local volume, and the
// engine loaders (the heavy tiers pulled on demand).

import type { EngineMount } from '@iris/core/engine';
import { STORAGE } from '#lib/storage.ts';
import type { DecodeTier } from '@iris/core/manifest-client';
import { mountTierA } from '@iris/core/tiers/tier-a-native';
import { stored, type Codec } from '#lib/stored.ts';

type WebkitVideo = HTMLVideoElement & { webkitEnterFullscreen?: () => void };

/** Fired on the stage when it enters or leaves the full screen kept in the page. */
export const PAGE_FULLSCREEN_CHANGE = 'iris-page-fullscreen';

const inPage = (target: HTMLElement) => target.dataset.pageFullscreen !== undefined;

export const isFullscreen = (target: HTMLElement): boolean => target.ownerDocument.fullscreenElement === target || inPage(target);

/** The stage over the whole viewport (PlayerStage styles it from the attribute); the root is
 * marked so the page behind neither scrolls nor shows its own colour beside the stage. */
function setPageFullscreen(target: HTMLElement, on: boolean) {
	if (inPage(target) === on) return;
	if (on) target.dataset.pageFullscreen = '';
	else delete target.dataset.pageFullscreen;
	const root = target.ownerDocument.documentElement;
	if (on) root.dataset.stageFullscreen = '';
	else delete root.dataset.stageFullscreen;
	target.dispatchEvent(new Event(PAGE_FULLSCREEN_CHANGE));
}

export const leavePageFullscreen = (target: HTMLElement) => setPageFullscreen(target, false);

/** `overlay`: subtitles are drawn on the stage (ASS / PGS), so the picture alone is not enough. */
export async function toggleFullscreen(target: HTMLElement | null | undefined, { overlay = false } = {}): Promise<void> {
	if (!target) return;
	const doc = target.ownerDocument;
	try {
		if (inPage(target)) setPageFullscreen(target, false);
		else if (doc.fullscreenElement === target) await doc.exitFullscreen();
		else if (typeof target.requestFullscreen === 'function') await target.requestFullscreen();
		else {
			// iPhone Safari has no element full screen, only its <video>'s, in the system player:
			// real full screen, but the Iris chrome and the subtitle canvas stay behind. With
			// drawn subtitles (or no <video> at all) the stage takes the viewport instead.
			const video = overlay ? null : target.querySelector<WebkitVideo>('video');
			if (video?.webkitEnterFullscreen) video.webkitEnterFullscreen();
			else setPageFullscreen(target, true);
		}
	} catch {
		// refused (an iframe, a browser setting): nothing to undo
	}
}

/** Keeps the screen on until the returned release: the lock is the document's and drops when
 * the tab is hidden, so it is taken again each time the page comes back. */
export function keepScreenAwake(doc: Document = document): () => void {
	const locks = doc.defaultView?.navigator.wakeLock;
	if (!locks) return () => undefined;
	let sentinel: WakeLockSentinel | null = null;
	let done = false;
	const take = async () => {
		if (doc.visibilityState !== 'visible') return;
		try {
			const s = await locks.request('screen');
			if (done) void s.release();
			else sentinel = s;
		} catch {
			// refused (battery saver, no permission): the screen follows the system's delay
		}
	};
	const onShow = () => void take();
	onShow();
	doc.addEventListener('visibilitychange', onShow);
	return () => {
		done = true;
		doc.removeEventListener('visibilitychange', onShow);
		void sentinel?.release().catch(() => undefined);
	};
}

const unit: Codec<number> = {
	read: (raw) => {
		const v = Number(raw);
		return Number.isFinite(v) ? Math.max(0, Math.min(1, v)) : undefined;
	},
	write: (v) => String(Math.max(0, Math.min(1, v)))
};

/** Volume is a device's, not the account's: kept in this browser, for every page that plays. */
const keptVolume = stored<number | undefined>(STORAGE.volume, undefined, unit as Codec<number | undefined>);
export const readStoredVolume = (): number | undefined => keptVolume.get();
export const writeStoredVolume = (v: number) => keptVolume.set(v);

type Loader = () => Promise<{ mount: EngineMount }>;

/** Tier A is the warm path and tiny; the others bring Mediabunny, hls.js or WASM on demand. */
const ENGINE_LOADERS: Record<DecodeTier, Loader> = {
	A: () => Promise.resolve({ mount: mountTierA }),
	B: () => import('@iris/core/tiers/tier-b-mse').then((m) => ({ mount: m.mountTierB })),
	C: () => import('@iris/core/tiers/tier-c-webcodecs').then((m) => ({ mount: m.mountTierC })),
	D: () => import('@iris/core/tiers/tier-c-webcodecs').then((m) => ({ mount: m.mountTierC })),
	E: () => import('@iris/core/tiers/tier-e-hevcjs').then((m) => ({ mount: m.mountTierE })),
	F: () => import('@iris/core/tiers/tier-f-hls').then((m) => ({ mount: m.mountTierF }))
};

/** Live engines are their own modules (HLS-following input, no seek machinery): C live is
 * WebCodecs + canvas with client-side broadcast concealment (the tuner path), B live the MSE
 * variant kept for A/B debugging via `?tier=B`. */
export function engineLoader(tier: DecodeTier, live: boolean): Loader | undefined {
	if (live && tier === 'C') return () => import('@iris/core/tiers/tier-c-live').then((m) => ({ mount: m.mountTierCLive }));
	if (live && tier === 'B') return () => import('@iris/core/tiers/tier-b-live').then((m) => ({ mount: m.mountTierBLive }));
	return ENGINE_LOADERS[tier];
}
