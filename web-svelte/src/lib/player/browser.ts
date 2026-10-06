// Small browser helpers the player shell shares: full screen, the device-local volume, and the
// engine loaders (the heavy tiers pulled on demand).

import type { EngineMount } from '@iris/core/engine';
import { STORAGE } from '#lib/storage.ts';
import type { DecodeTier } from '@iris/core/manifest-client';
import { mountTierA } from '@iris/core/tiers/tier-a-native';
import { stored, type Codec } from '#lib/stored.ts';

export async function toggleFullscreen(target: HTMLElement | null | undefined): Promise<void> {
	if (!target) return;
	const doc = target.ownerDocument;
	try {
		if (doc.fullscreenElement === target) await doc.exitFullscreen();
		else await target.requestFullscreen();
	} catch {
		// refused (an iframe, a browser setting): nothing to undo
	}
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
