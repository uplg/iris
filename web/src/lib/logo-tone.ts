/**
 * Pick the plate ("well") color to draw behind a channel logo from the
 * logo's own pixels. Channel logos are wild PNGs: black ink on transparency
 * vanishes on a dark plate, white ink vanishes on a light one — so the well
 * adapts: dark logo → light well, light logo → dark well, colorful /
 * mid-luminance → neutral gray that carries both.
 *
 * Logos arrive through the backend proxy (`/api/livetv/logo?…`) so they're
 * same-origin: the canvas is never tainted and no CORS request is made.
 * Any decode/read oddity still falls back to "neutral" — the analysis is
 * cosmetic and must never break the card.
 */

export type LogoTone = "light" | "neutral" | "dark";

/** Per-URL memo — a grid renders hundreds of cards and React Query refetches
 *  re-render them; one analysis per logo is plenty. */
const cache = new Map<string, LogoTone | Promise<LogoTone>>();

const SAMPLE_SIZE = 24;
const ALPHA_CUTOFF = 25; // ignore (near-)transparent pixels

function analyze(url: string): Promise<LogoTone> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = SAMPLE_SIZE;
        canvas.height = SAMPLE_SIZE;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          resolve("neutral");
          return;
        }
        ctx.drawImage(img, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
        const { data } = ctx.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
        let luma = 0;
        let count = 0;
        for (let i = 0; i < data.length; i += 4) {
          if (data[i + 3] < ALPHA_CUTOFF) continue;
          luma += 0.2126 * data[i] + 0.7152 * data[i + 1] + 0.0722 * data[i + 2];
          count += 1;
        }
        if (count === 0) {
          resolve("neutral");
          return;
        }
        const mean = luma / count / 255;
        resolve(mean < 0.38 ? "light" : mean > 0.62 ? "dark" : "neutral");
      } catch {
        resolve("neutral");
      }
    };
    img.onerror = () => resolve("neutral");
    img.src = url;
  });
}

/** Resolve (and memoize) the tone for a logo URL. */
export function logoTone(url: string): LogoTone | Promise<LogoTone> {
  const hit = cache.get(url);
  if (hit !== undefined) return hit;
  const pending = analyze(url).then((tone) => {
    cache.set(url, tone);
    return tone;
  });
  cache.set(url, pending);
  return pending;
}
