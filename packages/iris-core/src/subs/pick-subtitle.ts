/**
 * Cross-episode subtitle pick, shared rule with the Android TV client
 * (`SubtitlePick.kt`): keep the two in step when editing either.
 *
 * Per-file progress restores an exact track. Without one, the per-user
 * preferred LANGUAGE decides, and a language usually maps to several
 * tracks in a MULTi release: "Forcés" (forced, and often flagged default
 * by the muxer), "Complets", "SDH". Letting the player rank them, or
 * taking the first match, lands on the forced track: a handful of cues
 * for foreign-language lines, which reads as "subtitles off" to the
 * viewer, on every new episode (Mousetrap S01: ten times over).
 *
 * Ranking within the preferred language: non-forced before forced, and
 * among non-forced the plain track before an SDH/CC one; ties keep
 * source order. No track in the preferred language means no pick: never
 * force a different language onto the viewer.
 */

import type { SubtitleTrack } from "../manifest-client";

/**
 * ISO 639-2 (bibliographic and terminologic) to 639-1 for the languages
 * releases actually tag; anything else is lowercased with a region
 * suffix stripped. ffprobe hands back whatever the muxer wrote, and
 * `fre` / `fra` / `fr` all mean French, across releases of one series.
 */
const ISO_639_2_TO_1: Record<string, string> = {
  fre: "fr",
  fra: "fr",
  eng: "en",
  ger: "de",
  deu: "de",
  spa: "es",
  ita: "it",
  por: "pt",
  dut: "nl",
  nld: "nl",
  jpn: "ja",
  kor: "ko",
  chi: "zh",
  zho: "zh",
  rus: "ru",
  ara: "ar",
  pol: "pl",
  tur: "tr",
  swe: "sv",
  nor: "no",
  dan: "da",
  fin: "fi",
  cze: "cs",
  ces: "cs",
  gre: "el",
  ell: "el",
  hun: "hu",
  rum: "ro",
  ron: "ro",
  ukr: "uk",
  heb: "he",
  hin: "hi",
  tha: "th",
  vie: "vi",
  ind: "id",
  may: "ms",
  msa: "ms",
  per: "fa",
  fas: "fa",
  cat: "ca",
  baq: "eu",
  eus: "eu",
  glg: "gl",
  slo: "sk",
  slk: "sk",
  slv: "sl",
  hrv: "hr",
  srp: "sr",
  bul: "bg",
  lit: "lt",
  lav: "lv",
  est: "et",
  ice: "is",
  isl: "is",
  tgl: "tl",
  fil: "tl",
};

/** `null` for absent / unknown (`und`) tags. */
export function normalizeLang(code: string | null | undefined): string | null {
  if (!code) return null;
  const base = code.trim().toLowerCase().split(/[-_]/)[0] ?? "";
  if (!base || base === "und") return null;
  return ISO_639_2_TO_1[base] ?? base;
}

const SDH_TITLE = /\b(sdh|cc|hearing|malentendant|sourds?)\b/i;

function isSdh(track: SubtitleTrack): boolean {
  return SDH_TITLE.test(track.title ?? "");
}

export function pickPreferredSubtitle(
  tracks: readonly SubtitleTrack[],
  preferredLang: string | null | undefined,
): SubtitleTrack | null {
  const want = normalizeLang(preferredLang);
  if (!want) return null;
  const inLang = tracks.filter((t) => normalizeLang(t.lang) === want);
  if (inLang.length === 0) return null;
  const rank = (t: SubtitleTrack) => (t.forced ? 2 : isSdh(t) ? 1 : 0);
  return inLang.reduce((best, t) => (rank(t) < rank(best) ? t : best));
}
