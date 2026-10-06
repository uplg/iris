// The plate drawn behind a channel logo, picked from the logo's own pixels: whichever of the
// light and the dark plate contrasts more with the logo's mean luminance. No grey in between:
// a red, orange or grey logo all but vanished on it. Read from the tile's own <img> once it has loaded
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
		let lum = 0;
		let count = 0;
		for (let i = 0; i < data.length; i += 4) {
			if (data[i + 3] < ALPHA_CUTOFF) continue;
			lum += 0.2126 * linear(data[i]) + 0.7152 * linear(data[i + 1]) + 0.0722 * linear(data[i + 2]);
			count += 1;
		}
		if (count === 0) return 'neutral';
		return toneFor(lum / count);
	} catch {
		return 'neutral';
	}
}

const linear = (c: number) => {
	const v = c / 255;
	return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};

/** Relative luminance of the plates (cloud, night). */
const LIGHT_PLATE = 0.92;
const DARK_PLATE = 0.012;

/** The plate with the higher WCAG contrast against a logo of mean relative luminance `l`. */
export function toneFor(l: number): LogoTone {
	const onLight = (LIGHT_PLATE + 0.05) / (l + 0.05);
	const onDark = (l + 0.05) / (DARK_PLATE + 0.05);
	return onLight >= onDark ? 'light' : 'dark';
}
