// The plate drawn behind a channel logo, picked from the logo's own pixels: black ink vanishes
// on a dark plate, white ink on a light one, so a dark logo gets a light well, a light logo a
// dark one, a colorful one a neutral grey. Read from the tile's own <img> once it has loaded
// (lazy: a logo off screen is neither fetched nor decoded). Logos come through the backend
// proxy (same origin: the canvas is never tainted). Cosmetic: any oddity falls back to neutral.

export type LogoTone = 'light' | 'neutral' | 'dark';

/** One analysis per logo: a grid shows hundreds and the guide refetches. */
const cache = new Map<string, LogoTone>();

const SAMPLE = 24;
const ALPHA_CUTOFF = 25;

/** The tone already read for this logo, if any. */
export const knownTone = (url: string): LogoTone | undefined => cache.get(url);

/** The tone of a loaded logo, remembered under its url. */
export function readTone(url: string, img: HTMLImageElement): LogoTone {
	const hit = cache.get(url);
	if (hit) return hit;
	const tone = analyze(img);
	cache.set(url, tone);
	return tone;
}

function analyze(img: HTMLImageElement): LogoTone {
	try {
		const canvas = document.createElement('canvas');
		canvas.width = SAMPLE;
		canvas.height = SAMPLE;
		const ctx = canvas.getContext('2d');
		if (!ctx) return 'neutral';
		ctx.drawImage(img, 0, 0, SAMPLE, SAMPLE);
		const { data } = ctx.getImageData(0, 0, SAMPLE, SAMPLE);
		let luma = 0;
		let count = 0;
		for (let i = 0; i < data.length; i += 4) {
			if (data[i + 3] < ALPHA_CUTOFF) continue;
			luma += 0.2126 * data[i] + 0.7152 * data[i + 1] + 0.0722 * data[i + 2];
			count += 1;
		}
		if (count === 0) return 'neutral';
		const mean = luma / count / 255;
		return mean < 0.38 ? 'light' : mean > 0.62 ? 'dark' : 'neutral';
	} catch {
		return 'neutral';
	}
}
