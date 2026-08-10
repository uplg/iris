import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Play, TriangleAlert } from "lucide-react";
import DOMPurify from "dompurify";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  ApiError,
  searchDetails,
  torrents,
  type FilePreview,
  type MediaInfoSummary,
  type TorrentDetails,
  type TorrentPreview,
} from "@/lib/api";
import { formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";

/** Above this total size the grab needs an explicit second click — born
 *  from a user grabbing a complete-series pack right after grabbing the
 *  one season they actually wanted. Big packs hog the shared disk and
 *  get everyone's library GC-evicted sooner. */
const HUGE_GRAB_BYTES = 50 * 1024 ** 3;

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  providerId: string | null;
  externalId: string | null;
  initialTitle?: string;
  tmdbId?: number | null;
  /** Seeder count from the search hit. 0 = dead torrent → grab disabled (its
   *  pieces would never fully assemble). null/≥1 grabs normally (1 seeder is
   *  often plenty; we never warn, only block a confirmed 0). */
  seeders?: number | null;
  /** Server-flagged dedup: the result's SCENE identity matches an
   *  episode already on disk. UI surfaces a "you already have this"
   *  banner and promotes "Play existing" over "Download anyway",
   *  preventing the surprisingly-common second ingest of the same
   *  episode under a different release group. */
  alreadyInLibrary?: boolean;
  libraryInfohash?: string | null;
  libraryFileIdx?: number | null;
};

export function PreviewDialog({
  open,
  onOpenChange,
  providerId,
  externalId,
  initialTitle,
  tmdbId,
  seeders = null,
  alreadyInLibrary = false,
  libraryInfohash = null,
  libraryFileIdx = null,
}: Props) {
  const navigate = useNavigate();
  const [preview, setPreview] = useState<TorrentPreview | null>(null);
  const [details, setDetails] = useState<TorrentDetails | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pickedIdx, setPickedIdx] = useState<number | null>(null);
  const [ingesting, setIngesting] = useState(false);
  const [showNfo, setShowNfo] = useState(false);
  /** Set when the server answered `409 duplicate_in_library`: the
   *  message lists the copies already on disk. The dialog switches the
   *  CTA to an explicit "Download another copy" that retries with
   *  `allow_duplicate`. */
  const [dupMessage, setDupMessage] = useState<string | null>(null);
  /** Two-step guard for >50 GB releases: the first Play click arms the
   *  warning instead of ingesting; only the explicit confirm proceeds. */
  const [hugeWarned, setHugeWarned] = useState(false);

  useEffect(() => {
    if (!open || !providerId || !externalId) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPreview(null);
    setDetails(null);
    setPickedIdx(null);
    setShowNfo(false);
    setDupMessage(null);
    setHugeWarned(false);
    // Fire both requests in parallel — preview is required (drives the
    // file picker + ingest), details is best-effort (some providers
    // don't expose them and we want the dialog to still open).
    const previewP = torrents.preview(providerId, externalId);
    const detailsP = searchDetails.get(providerId, externalId).catch(() => null);
    void Promise.all([previewP, detailsP])
      .then(([p, d]) => {
        if (cancelled) return;
        setPreview(p);
        setDetails(d);
        const auto = pickAutoFile(p.files);
        if (auto != null) setPickedIdx(auto);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(e instanceof ApiError ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, providerId, externalId]);

  const sortedFiles = useMemo(() => {
    if (!preview) return [] as FilePreview[];
    // Videos first, then episodes in SCENE order (E01, E02, …) so a season
    // pack reads naturally and the auto-selected episode 1 sits on top.
    // Files without an SxxExx marker (movies / extras) fall back to
    // largest-first; non-video trails.
    return [...preview.files].sort((a, b) => {
      if (a.is_video !== b.is_video) return a.is_video ? -1 : 1;
      const sa = parseSe(a.path);
      const sb = parseSe(b.path);
      if (sa && sb) return sa.season - sb.season || sa.episode - sb.episode;
      if (sa) return -1;
      if (sb) return 1;
      return b.size_bytes - a.size_bytes;
    });
  }, [preview]);

  // Dead-torrent guard: prefer the freshly-loaded details' seeders, fall back
  // to the search-hit count. A confirmed 0 blocks the grab (its pieces would
  // never fully assemble); unknown / ≥1 grabs normally.
  const dead = (details?.seeders ?? seeders) === 0;

  // RAR'd scene release: the server refuses the grab (409 archive_only)
  // because there is nothing Iris could stream. Block the CTA up front
  // with an explanation instead of letting the user hit the wall.
  const notStreamable = preview != null && !preview.streamable;

  const huge = preview != null && preview.total_size_bytes > HUGE_GRAB_BYTES;

  const onPlay = async (allowDuplicate = false) => {
    if (dead || notStreamable || !preview || pickedIdx == null || !providerId || !externalId) {
      return;
    }
    if (huge && !hugeWarned) {
      setHugeWarned(true);
      return;
    }
    setIngesting(true);
    setError(null);
    try {
      const res = await torrents.ingest(providerId, externalId, tmdbId ?? null, allowDuplicate);
      onOpenChange(false);
      navigate({
        to: "/watch/$infohash/$idx",
        params: { infohash: res.snapshot.infohash, idx: String(pickedIdx) },
      });
    } catch (e) {
      if (e instanceof ApiError && e.code === "duplicate_in_library") {
        setDupMessage(e.message);
      } else {
        setError(e instanceof ApiError ? e.message : String(e));
      }
    } finally {
      setIngesting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-5xl w-[90vw] sm:w-full overflow-hidden">
        {/* `!max-w-5xl` overrides shadcn's default `sm:max-w-lg` baked into
            DialogContent — without the `!` Tailwind keeps the smaller
            breakpoint cap. The `w-[90vw]` ensures we still feel roomy on
            mid-size laptops where 5xl (=64rem ≈ 1024 px) might be capped
            by a smaller viewport. */}
        <DialogHeader className="min-w-0">
          <DialogTitle
            className="break-words"
            title={preview?.name ?? details?.title ?? initialTitle ?? undefined}
          >
            {preview?.name ?? details?.title ?? initialTitle ?? "Loading…"}
          </DialogTitle>
          <DialogDescription>
            {preview && (
              <span>
                {preview.files.length} file{preview.files.length > 1 ? "s" : ""} ·{" "}
                {formatSize(preview.total_size_bytes)}
                {details?.uploader && (
                  <span className="ml-2 text-muted-foreground">
                    · uploaded by {details.uploader}
                    {details.age && ` (${details.age})`}
                  </span>
                )}
              </span>
            )}
            {!preview && "Reading metadata…"}
          </DialogDescription>
          {details &&
            ((details.tags?.length ?? 0) > 0 || details.freeleech || details.exclusive) && (
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                {details.freeleech && (
                  <Badge
                    className="bg-emerald-500/90 text-[10px] uppercase text-white"
                    title="Freeleech: this download doesn't count against our ratio on the tracker"
                  >
                    Freeleech
                  </Badge>
                )}
                {details.exclusive && (
                  <Badge className="bg-amber-500/90 text-[10px] uppercase text-white">
                    Exclusive
                  </Badge>
                )}
                {(details.tags ?? []).slice(0, 6).map((t) => (
                  <Badge key={t} variant="outline" className="text-[10px]">
                    {t}
                  </Badge>
                ))}
              </div>
            )}
        </DialogHeader>

        {loading && <p className="text-sm text-muted-foreground">Loading…</p>}
        {error && <p className="text-sm text-destructive">{error}</p>}

        <div className="grid max-h-[60vh] gap-4 overflow-y-auto">
          {/* Facts grid — parsed MediaInfo. Hidden when no NFO available. */}
          {details?.media_info && <FactsGrid mi={details.media_info} />}

          {/* Description — dispatched to the right renderer based on
              the provider's declared format. */}
          {details?.description && (
            <div className="rounded-md border border-border bg-card/30 p-3 text-sm">
              <Description
                source={details.description}
                format={details.description_format ?? "bbcode"}
              />
            </div>
          )}

          {/* File picker (existing). */}
          {preview && (
            <div className="rounded-md border border-border">
              <div className="border-b border-border bg-muted/30 px-3 py-2 text-[11px] uppercase tracking-wide text-muted-foreground">
                Files · pick what to play
              </div>
              <ul className="divide-y divide-border">
                {sortedFiles.map((f) => {
                  const selected = pickedIdx === f.index;
                  return (
                    <li
                      key={f.index}
                      className={cn(
                        "flex cursor-pointer items-center gap-3 px-3 py-2 text-sm transition",
                        selected ? "bg-accent text-accent-foreground" : "hover:bg-muted/40",
                      )}
                      onClick={() => setPickedIdx(f.index)}
                    >
                      <input
                        type="radio"
                        checked={selected}
                        onChange={() => setPickedIdx(f.index)}
                        className="accent-current"
                      />
                      <div className="min-w-0 flex-1">
                        <div className="break-all font-mono text-xs" title={f.path}>
                          {f.path}
                        </div>
                        <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
                          <span>{formatSize(f.size_bytes)}</span>
                          {f.is_video ? (
                            <Badge variant="secondary" className="text-[10px]">
                              video
                            </Badge>
                          ) : f.extension ? (
                            <span className="text-[10px] uppercase tracking-wide">
                              {f.extension}
                            </span>
                          ) : null}
                        </div>
                      </div>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}

          {/* NFO raw — power-user collapsible. */}
          {details?.nfo && (
            <details
              open={showNfo}
              onToggle={(e) => setShowNfo((e.target as HTMLDetailsElement).open)}
              className="rounded-md border border-border bg-card/30"
            >
              <summary className="cursor-pointer px-3 py-2 text-xs uppercase tracking-wide text-muted-foreground">
                Raw NFO (MediaInfo)
              </summary>
              <pre className="max-h-80 overflow-auto border-t border-border bg-background/40 p-3 font-mono text-[11px] leading-relaxed">
                {details.nfo}
              </pre>
            </details>
          )}
        </div>

        {alreadyInLibrary &&
          libraryInfohash != null && (
            // Server-side dedup hit — surface a clear banner so the
            // user understands why the Play CTA points at the existing
            // file rather than re-downloading. Without it people kept
            // accidentally ingesting the same episode twice via a
            // different release.
            <div className="mt-2 rounded-md border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-300">
              You already have this episode in your library. Playing the existing file; use{" "}
              <span className="font-medium">Download anyway</span> below only if you want a
              different release.
            </div>
          )}

        {dead && (
          // Dead-torrent guard: the chosen release has no seeders, so the
          // grab is disabled. We never warn on a merely-low count (1 seeder is
          // often plenty) — only a confirmed 0 blocks.
          <div className="mt-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            Dead torrent: no seeders. This release can't be downloaded; try another.
          </div>
        )}

        {notStreamable && (
          <div className="mt-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            RAR-packed release: the video sits inside archives Iris can't stream. Pick an unrar'd
            release (a plain .mkv / .mp4).
          </div>
        )}

        {hugeWarned &&
          !dupMessage &&
          preview && (
            // Armed by the first Play click on a >50 GB release. Loud on
            // purpose: complete-series packs grabbed "just in case" hog the
            // shared disk and get everyone's library evicted sooner.
            <div className="mt-2 flex items-start gap-2.5 rounded-md border-2 border-warn/60 bg-warn/10 px-3 py-2.5 text-sm text-warn">
              <TriangleAlert className="mt-0.5 size-4 shrink-0" />
              <div>
                <p className="font-semibold">
                  This release is {formatSize(preview.total_size_bytes)} — are you really sure you
                  want it?
                </p>
                <p className="mt-1 text-xs opacity-90">
                  Huge packs (complete series, full box sets) eat the shared disk and get everyone's
                  library cleaned up sooner. If you only want one season or episode, grab that
                  release instead.
                </p>
              </div>
            </div>
          )}

        {dupMessage && (
          <div className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-300">
            {dupMessage}. Download another copy anyway?
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          {dupMessage ? (
            <Button variant="secondary" onClick={() => onPlay(true)} disabled={ingesting}>
              {ingesting ? "Starting…" : "Download another copy"}
            </Button>
          ) : alreadyInLibrary && libraryInfohash != null && libraryFileIdx != null ? (
            <>
              <Button
                variant="secondary"
                onClick={() => onPlay()}
                disabled={pickedIdx == null || ingesting || !preview || dead || notStreamable}
              >
                {ingesting
                  ? "Starting…"
                  : hugeWarned && preview
                    ? `Yes, download ${formatSize(preview.total_size_bytes)}`
                    : "Download anyway"}
              </Button>
              <Button
                onClick={() => {
                  onOpenChange(false);
                  navigate({
                    to: "/watch/$infohash/$idx",
                    params: { infohash: libraryInfohash, idx: String(libraryFileIdx) },
                  });
                }}
              >
                <Play className="size-4" />
                Play existing
              </Button>
            </>
          ) : hugeWarned && preview ? (
            <Button
              variant="destructive"
              onClick={() => onPlay()}
              disabled={pickedIdx == null || ingesting || dead || notStreamable}
            >
              <TriangleAlert className="size-4" />
              {ingesting ? "Starting…" : `Yes, download ${formatSize(preview.total_size_bytes)}`}
            </Button>
          ) : (
            <Button
              onClick={() => onPlay()}
              disabled={pickedIdx == null || ingesting || !preview || dead || notStreamable}
            >
              <Play className="size-4" />
              {dead
                ? "Dead torrent"
                : notStreamable
                  ? "Not streamable"
                  : ingesting
                    ? "Starting…"
                    : "Play"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** SCENE `SxxExx` marker from a file's basename (handles `S01E02`,
 *  `S1E2`, `S01.E02`, and the fleuve `S01E1156`). `null` when absent. */
function parseSe(path: string): { season: number; episode: number } | null {
  const base = path.split("/").pop() ?? path;
  const m = /\bS(\d{1,4})[._ -]*E(\d{1,4})\b/i.exec(base);
  if (!m) return null;
  return { season: Number.parseInt(m[1], 10), episode: Number.parseInt(m[2], 10) };
}

/** SCENE samples are tagged video files we must never auto-select —
 *  matches the backend's `is_main_video_file` exclusion. */
function isSampleFile(path: string): boolean {
  const p = path.toLowerCase();
  return p.includes("/sample/") || p.includes(".sample.") || /\bsample\b/.test(p);
}

function pickAutoFile(files: FilePreview[]): number | null {
  const videos = files.filter((f) => f.is_video && !isSampleFile(f.path));
  const pool = videos.length ? videos : files;
  if (!pool.length) return null;
  // Season pack: land on the FIRST episode (lowest season, then episode)
  // so the post-grab "Play" opens episode 1 — not whatever episode happens
  // to have the biggest file. The Watch page then chains to the next.
  const withSe = pool
    .map((f) => ({ f, se: parseSe(f.path) }))
    .filter((x): x is { f: FilePreview; se: { season: number; episode: number } } => x.se != null);
  if (withSe.length > 0) {
    return withSe.reduce((best, x) =>
      x.se.season < best.se.season ||
      (x.se.season === best.se.season && x.se.episode < best.se.episode)
        ? x
        : best,
    ).f.index;
  }
  // Movie / single unparseable file: largest video wins.
  return pool.reduce((best, f) => (f.size_bytes > best.size_bytes ? f : best), pool[0])!.index;
}

// FactsGrid — structured MediaInfo summary.

function FactsGrid({ mi }: { mi: MediaInfoSummary }) {
  // `audio`/`subtitles` are `#[serde(default)]` server-side (always sent, but
  // the schema marks them optional), so normalise to arrays.
  const audio = mi.audio ?? [];
  const subtitles = mi.subtitles ?? [];
  if (!mi.video && audio.length === 0 && subtitles.length === 0) return null;
  return (
    <div className="grid gap-3 rounded-md border border-border bg-card/30 p-3 text-sm">
      {mi.video && (
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="font-semibold uppercase tracking-wide text-muted-foreground">Video</span>
          {mi.video.codec && <Badge variant="outline">{mi.video.codec}</Badge>}
          {mi.video.resolution && <Badge variant="outline">{mi.video.resolution}</Badge>}
          {mi.video.fps != null && <Badge variant="outline">{mi.video.fps.toFixed(2)}fps</Badge>}
          {mi.video.bitrate_kbps != null && (
            <Badge variant="outline">{mi.video.bitrate_kbps.toLocaleString()} kb/s</Badge>
          )}
          {mi.video.hdr && <Badge className="bg-amber-500/80 text-white">{mi.video.hdr}</Badge>}
          {mi.video.duration_secs && (
            <span className="text-muted-foreground">· {formatRuntime(mi.video.duration_secs)}</span>
          )}
        </div>
      )}
      {audio.length > 0 && (
        <div className="flex flex-wrap items-start gap-2 text-xs">
          <span className="mt-0.5 font-semibold uppercase tracking-wide text-muted-foreground">
            Audio
          </span>
          <div className="flex flex-1 flex-wrap gap-1.5">
            {audio.map((a, i) => (
              <span
                key={i}
                className="inline-flex items-center gap-1 rounded-md border border-border px-2 py-0.5"
              >
                <span className="font-medium">{a.lang ?? "?"}</span>
                {a.commercial_name ? (
                  <span className="text-muted-foreground">{a.commercial_name}</span>
                ) : a.codec ? (
                  <span className="text-muted-foreground">{a.codec}</span>
                ) : null}
                {a.channels && (
                  <span className="text-muted-foreground">{channelLabel(a.channels)}</span>
                )}
                {a.default && (
                  <Badge variant="secondary" className="text-[10px]">
                    def
                  </Badge>
                )}
              </span>
            ))}
          </div>
        </div>
      )}
      {subtitles.length > 0 && (
        <div className="flex flex-wrap items-start gap-2 text-xs">
          <span className="mt-0.5 font-semibold uppercase tracking-wide text-muted-foreground">
            Subtitles
          </span>
          <div className="flex flex-1 flex-wrap gap-1.5">
            {subtitles.map((s, i) => (
              <span
                key={i}
                className="inline-flex items-center gap-1 rounded-md border border-border px-2 py-0.5"
              >
                <span className="font-medium">{s.lang ?? "?"}</span>
                {s.format && <span className="text-muted-foreground">{s.format}</span>}
                {s.forced && (
                  <Badge variant="secondary" className="text-[10px]">
                    forced
                  </Badge>
                )}
                {s.title?.toLowerCase().includes("sdh") && (
                  <Badge variant="secondary" className="text-[10px]">
                    SDH
                  </Badge>
                )}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function channelLabel(n: number): string {
  switch (n) {
    case 1:
      return "1.0";
    case 2:
      return "2.0";
    case 6:
      return "5.1";
    case 8:
      return "7.1";
    default:
      return `${n}ch`;
  }
}

function formatRuntime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h${m.toString().padStart(2, "0")}`;
  return `${m}min`;
}

// Description dispatcher — pick the right renderer per provider format.
//
// torr9 ships BBCode, c411 ships HTML. The backend declares which via
// `description_format` on `TorrentDetails`; we dispatch here so the
// renderers stay small and decoupled.

function Description({
  source,
  format,
}: {
  source: string;
  format: NonNullable<TorrentDetails["description_format"]>;
}) {
  if (format === "html") return <SanitizedHtml source={source} />;
  if (format === "plain") {
    return (
      <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">
        {source}
      </pre>
    );
  }
  return <BBCode source={source} />;
}

// Sanitised HTML renderer (c411 + any future HTML indexers)
//
// Indexer descriptions are untrusted markup — sanitise with DOMPurify
// before injection. The configured allow-list keeps c411's rich layout
// (headings, tables, inline images, links) and drops every code path
// that can run JS (scripts, event handlers, javascript: URLs). External
// links are post-processed to open in a new tab with `noopener`.

// DOMPurify hooks are global — register once at module load so we don't
// thrash add/remove across renders. The hook is idempotent and only
// touches `<a>` elements, so it can't interfere with other call sites
// (currently there are none in the app).
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node instanceof Element && node.tagName === "A" && node.hasAttribute("href")) {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noopener noreferrer");
  }
});

const HTML_ALLOWED_TAGS = [
  "a",
  "b",
  "blockquote",
  "br",
  "code",
  "div",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "i",
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "span",
  "strong",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
];

const HTML_ALLOWED_ATTR = [
  "href",
  "src",
  "alt",
  "class",
  "id",
  "title",
  "loading",
  "referrerpolicy",
  "target",
  "rel",
];

function sanitizeIndexerHtml(source: string): string {
  return DOMPurify.sanitize(source, {
    ALLOWED_TAGS: HTML_ALLOWED_TAGS,
    ALLOWED_ATTR: HTML_ALLOWED_ATTR,
    FORBID_TAGS: ["script", "style", "iframe", "form", "input", "button"],
    ALLOW_DATA_ATTR: false,
  });
}

function SanitizedHtml({ source }: { source: string }) {
  const html = useMemo(() => sanitizeIndexerHtml(source), [source]);
  return (
    <div
      // Tailwind arbitrary-variant styling keeps headings legible inside
      // the dialog and trims runaway images down to dialog height.
      className={cn(
        "space-y-2 leading-relaxed",
        "[&_h1]:mt-3 [&_h1]:text-base [&_h1]:font-semibold",
        "[&_h2]:mt-3 [&_h2]:text-sm [&_h2]:font-semibold",
        "[&_h3]:mt-2 [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:uppercase [&_h3]:tracking-wide",
        "[&_p]:my-1",
        "[&_a]:text-primary [&_a]:underline hover:[&_a]:no-underline",
        "[&_img]:my-1 [&_img]:inline-block [&_img]:max-h-40 [&_img]:rounded-sm",
        "[&_table]:my-2 [&_table]:w-full [&_table]:border-collapse [&_table]:text-xs",
        "[&_th]:bg-muted [&_th]:px-2 [&_th]:py-1 [&_th]:text-left [&_th]:font-semibold",
        "[&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1",
        "[&_em]:italic [&_strong]:font-semibold",
      )}
      // safe: sanitised by DOMPurify above with a strict allow-list.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

// BBCode renderer
//
// Targeted at the tags torr9 emits: [b], [i], [center], [size=N],
// [color=#xxx], [url=X]Y[/url], [img]X[/img]. We do a small recursive-
// descent parse rather than a regex-soup: regex doesn't handle nesting,
// and torr9 nests heavily ([center][size][b][color]…). Unknown tags
// fall through as plain text so we never lose content.

function BBCode({ source }: { source: string }) {
  const cleaned = useMemo(() => stripCosmeticSeparators(source), [source]);
  const tree = useMemo(() => parseBBCode(cleaned), [cleaned]);
  return <div className="space-y-1 leading-relaxed">{renderNodes(tree)}</div>;
}

type BBNode =
  | { type: "text"; value: string }
  | { type: "tag"; name: string; arg: string | null; children: BBNode[] };

// Tag forms we accept:
//   `[b]` / `[/b]`            — bare
//   `[color=#3d85c6]`         — value attached to name with `=`
//   `[img scale=30%]URL[/img]`— extra attributes separated by space
//                                (we don't consume them but mustn't choke)
//
// Capture groups: (1) optional `/`, (2) tag name, (3) optional `=value`
// payload (for `[tag=value]` form). Trailing space-separated attrs are
// matched non-capturing and discarded.
const TAG_RE = /\[(\/?)([a-zA-Z]+)(?:=([^\] ]*))?(?:\s+[^\]]*)?\]/g;

function parseBBCode(input: string): BBNode[] {
  // Stack-based parser. Push a frame for each opening tag, pop on
  // matching close. Mismatched closes are treated as literal text so a
  // typo'd `[/colorr]` doesn't lose all subsequent content.
  type Frame = { name: string; arg: string | null; children: BBNode[] };
  const root: Frame = { name: "__root__", arg: null, children: [] };
  const stack: Frame[] = [root];
  let cursor = 0;
  for (const m of input.matchAll(TAG_RE)) {
    const start = m.index ?? 0;
    if (start > cursor) {
      const text = input.slice(cursor, start);
      stack[stack.length - 1].children.push({ type: "text", value: text });
    }
    const closing = m[1] === "/";
    const name = m[2].toLowerCase();
    const arg = m[3] ?? null;
    if (closing) {
      // Pop until we find the matching opener — tolerate unbalanced.
      const idx = [...stack].reverse().findIndex((f) => f.name === name);
      if (idx >= 0) {
        const popTo = stack.length - 1 - idx;
        for (let i = stack.length - 1; i > popTo; i--) {
          // Re-emit unclosed-tag content as text so nothing is lost.
          stack[i - 1].children.push({
            type: "tag",
            name: stack[i].name,
            arg: stack[i].arg,
            children: stack[i].children,
          });
        }
        stack.length = popTo + 1;
        const closed = stack.pop()!;
        stack[stack.length - 1].children.push({
          type: "tag",
          name: closed.name,
          arg: closed.arg,
          children: closed.children,
        });
      } else {
        // Unmatched close — keep as text.
        stack[stack.length - 1].children.push({ type: "text", value: m[0] });
      }
    } else {
      stack.push({ name, arg, children: [] });
    }
    cursor = start + m[0].length;
  }
  if (cursor < input.length) {
    stack[stack.length - 1].children.push({ type: "text", value: input.slice(cursor) });
  }
  // Anything left on the stack above root: unclosed openings, render as
  // text so we don't drop content.
  while (stack.length > 1) {
    const open = stack.pop()!;
    stack[stack.length - 1].children.push({
      type: "text",
      value: `[${open.name}${open.arg ? `=${open.arg}` : ""}]`,
    });
    for (const c of open.children) stack[stack.length - 1].children.push(c);
  }
  return root.children;
}

function renderNodes(nodes: BBNode[]): ReactNode {
  return nodes.map((n, i) => {
    if (n.type === "text") {
      // Preserve newlines as <br/>.
      const parts = n.value.split("\n");
      return parts.map((p, j) => (
        <span key={`${i}-${j}`}>
          {p}
          {j < parts.length - 1 && <br />}
        </span>
      ));
    }
    return renderTag(n, i);
  });
}

function renderTag(n: Extract<BBNode, { type: "tag" }>, key: number): ReactNode {
  const inner = renderNodes(n.children);
  switch (n.name) {
    case "b":
      return <strong key={key}>{inner}</strong>;
    case "i":
      return <em key={key}>{inner}</em>;
    case "u":
      return <u key={key}>{inner}</u>;
    case "center":
      return (
        <div key={key} className="text-center">
          {inner}
        </div>
      );
    case "size": {
      // Ignore the actual size (everything in the dialog should be
      // body-sized for legibility); only "1" stays small as a hint.
      const small = n.arg && parseInt(n.arg, 10) <= 2;
      return (
        <span key={key} className={cn(small && "text-[10px] text-muted-foreground")}>
          {inner}
        </span>
      );
    }
    case "color":
      return (
        <span key={key} style={n.arg ? { color: n.arg } : undefined}>
          {inner}
        </span>
      );
    case "url":
      return (
        <a
          key={key}
          href={safeBbcodeHref(n.arg)}
          target="_blank"
          rel="noopener noreferrer"
          className="text-primary underline hover:no-underline"
        >
          {inner}
        </a>
      );
    case "img": {
      // [img]url[/img] — children should be a single text node with the URL.
      // http(s) only: the URL is tracker-authored; anything else is either
      // dead (javascript: in img src) or weird enough to drop.
      const flat = nodesToText(n.children).trim();
      if (!flat || !/^https?:\/\//i.test(flat)) return null;
      return (
        <img
          key={key}
          src={flat}
          alt=""
          loading="lazy"
          className="my-1 inline-block max-h-32 rounded-sm"
        />
      );
    }
    default:
      return <span key={key}>{inner}</span>;
  }
}

/** BBCode `[url=…]` hrefs are tracker-authored. The HTML description
 *  path is DOMPurify-sanitised, but BBCode is rendered by us — keep
 *  only schemes that can't execute script (`javascript:` in an href is
 *  a click-to-XSS). Anything else collapses to an inert anchor. */
function safeBbcodeHref(raw: string | null | undefined): string {
  const v = (raw ?? "").trim();
  return /^(https?:|magnet:)/i.test(v) ? v : "#";
}

function nodesToText(nodes: BBNode[]): string {
  return nodes.map((n) => (n.type === "text" ? n.value : nodesToText(n.children))).join("");
}

/** Drop lines that are pure decoration (long runs of unicode separator
 *  glyphs). torr9 descriptions use ━━━ / · runs as section dividers
 *  which read like spam in a normal-typography context. */
function stripCosmeticSeparators(input: string): string {
  return input
    .split("\n")
    .filter((line) => {
      const stripped = line.replace(/\[[^\]]+\]/g, "").trim();
      if (!stripped) return true; // keep blank lines for spacing
      // Lines that are 95%+ decorative chars get axed.
      const decorative = stripped.match(/[━—–·•⋯]/g)?.length ?? 0;
      return decorative / stripped.length < 0.5;
    })
    .join("\n");
}
