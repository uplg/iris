/** `H:MM:SS` (or `M:SS` under an hour) — playback position display, shared
 *  by Continue Watching and the Watch History page. */
export function formatTimecode(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Short "2m ago" / "just now" relative time for recent (sub-day) activity —
 *  finer-grained than {@link formatRelative}, which only resolves to whole
 *  days. Recompute on each render (e.g. driven by a query's
 *  `refetchInterval`) rather than with a timer of your own. */
export function formatRecentTime(iso: string): string {
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 10) return "just now";
  if (secs < 60) return `${Math.floor(secs)}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

export function formatSize(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

// Release tokens that mark the end of the human title in a SCENE name:
// year, resolution, source, codec, audio, language, season/episode,
// and the usual edition/flag words. Anchored — must match a whole token.
const SCENE_STOP =
  /^(19\d{2}|20\d{2}|s\d{1,2}(e\d{1,3})?|e\d{1,3}|\d{3,4}p|web|web-?dl|webrip|bluray|blu-?ray|brrip|bdrip|hdtv|dvdrip|dvd|remux|x264|x265|h264|h265|hevc|avc|xvid|divx|aac\d?|ac3|eac3|dts(-?hd)?(-?ma)?|ddp?\d?|truehd|atmos|flac|multi|vff|vfi|vof|vfq|vostfr|vost|vo|vf|french|truefrench|english|hdr|hdr10\+?|dovi|dv|10bit|8bit|repack|proper|internal|limited|uncut|unrated|extended|imax|complete|integrale)$/i;

/**
 * Roughly clean a raw SCENE release name for display when we have no
 * *verified* TMDB title. Cuts at the first release token and joins the
 * leading words; appends a trailing year if one delimited the title.
 * Best-effort — not a real parser, just enough to keep a hero/card
 * readable instead of dumping
 * "Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv".
 */
export function prettySceneName(raw: string): string {
  const noExt = raw.replace(/\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv|srt|nfo)$/i, "");
  // Split on dots/underscores/spaces only — NOT hyphens, so titles like
  // "Spider-Man" survive (and the trailing "-GROUP" is never reached
  // because we stop at an earlier release token anyway).
  const tokens = noExt.split(/[._\s]+/).filter(Boolean);

  const title: string[] = [];
  let year: string | null = null;
  for (const t of tokens) {
    if (SCENE_STOP.test(t)) {
      if (/^(19|20)\d{2}$/.test(t) && title.length > 0) year = t;
      break;
    }
    title.push(t);
  }

  // First token was already a release marker (or nothing parsed) — fall
  // back to a plain de-dotted form rather than an empty string.
  if (title.length === 0) return tokens.join(" ");
  const name = title.join(" ");
  return year ? `${name} (${year})` : name;
}

export function formatRelative(iso: string | null | undefined): string {
  if (!iso) return "";
  const then = new Date(iso).getTime();
  const days = Math.floor((Date.now() - then) / 86_400_000);
  if (days < 1) return "today";
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.floor(months / 12)}y ago`;
}
