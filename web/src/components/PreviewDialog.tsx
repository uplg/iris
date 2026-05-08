import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { Play } from "lucide-react";
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
import { ApiError, torrents, type TorrentPreview, type FilePreview } from "@/lib/api";
import { formatSize } from "@/lib/format";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  providerId: string | null;
  externalId: string | null;
  initialTitle?: string;
  tmdbId?: number | null;
};

export function PreviewDialog({ open, onOpenChange, providerId, externalId, initialTitle, tmdbId }: Props) {
  const navigate = useNavigate();
  const [preview, setPreview] = useState<TorrentPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pickedIdx, setPickedIdx] = useState<number | null>(null);
  const [ingesting, setIngesting] = useState(false);

  useEffect(() => {
    if (!open || !providerId || !externalId) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPreview(null);
    setPickedIdx(null);
    torrents
      .preview(providerId, externalId)
      .then((p) => {
        if (cancelled) return;
        setPreview(p);
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
    return [...preview.files].sort((a, b) => {
      if (a.is_video !== b.is_video) return a.is_video ? -1 : 1;
      return b.size_bytes - a.size_bytes;
    });
  }, [preview]);

  const onPlay = async () => {
    if (!preview || pickedIdx == null || !providerId || !externalId) return;
    setIngesting(true);
    setError(null);
    try {
      const res = await torrents.ingest(providerId, externalId, tmdbId ?? null);
      onOpenChange(false);
      navigate(`/watch/${res.snapshot.infohash}/${pickedIdx}`);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      setIngesting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl overflow-hidden">
        <DialogHeader className="min-w-0">
          <DialogTitle className="break-words" title={preview?.name ?? initialTitle ?? undefined}>
            {preview?.name ?? initialTitle ?? "Loading…"}
          </DialogTitle>
          <DialogDescription>
            {preview
              ? `${preview.files.length} file${preview.files.length > 1 ? "s" : ""} · ${formatSize(preview.total_size_bytes)}`
              : "Reading torrent metadata…"}
          </DialogDescription>
        </DialogHeader>

        {loading && <p className="text-sm text-muted-foreground">Loading…</p>}
        {error && <p className="text-sm text-destructive">{error}</p>}

        {preview && (
          <div className="max-h-[50vh] overflow-y-auto rounded-md border border-border">
            <ul className="divide-y divide-border">
              {sortedFiles.map((f) => {
                const selected = pickedIdx === f.index;
                return (
                  <li
                    key={f.index}
                    className={`flex cursor-pointer items-center gap-3 px-3 py-2 text-sm transition ${
                      selected ? "bg-accent text-accent-foreground" : "hover:bg-muted/40"
                    }`}
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
                          <span className="text-[10px] uppercase tracking-wide">{f.extension}</span>
                        ) : null}
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={onPlay} disabled={pickedIdx == null || ingesting || !preview}>
            <Play className="size-4" />
            {ingesting ? "Starting…" : "Play"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function pickAutoFile(files: FilePreview[]): number | null {
  // Prefer the largest video file (typical season pack: pick biggest single
  // episode). If no video, take the largest file overall.
  const videos = files.filter((f) => f.is_video);
  const pool = videos.length ? videos : files;
  if (!pool.length) return null;
  return pool.reduce((best, f) => (f.size_bytes > best.size_bytes ? f : best), pool[0])!.index;
}
