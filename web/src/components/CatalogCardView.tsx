import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useNavigate } from "react-router";

import { MediaCard } from "@/components/MediaCard";
import { PreviewDialog } from "@/components/PreviewDialog";
import { Tag } from "@/components/Tag";
import { me as meApi, type CatalogCard } from "@/lib/api";

/**
 * A "For You" card. A followed series with new episodes opens its collection.
 * Everything else opens the SAME preview dialog as a search hit, so the user
 * sees the files / MediaInfo / quality before committing a download:
 *   - a rolling-window card carries its recommended-best release
 *     (`provider_id`/`external_id`) → preview that release directly;
 *   - a lazy recommendation (no resolved release yet) → fall back to a title
 *     search so the user picks + previews a release.
 * Recommendation candidates get a "not interested" dismiss button on hover.
 */
export function CatalogCardView({ card }: { card: CatalogCard }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [preview, setPreview] = useState(false);

  const dismiss = useMutation({
    mutationFn: () => meApi.dismissForYou(card.catalog_id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["for-you"] });
      void qc.invalidateQueries({ queryKey: ["for-you-page"] });
    },
  });

  const isSeries = card.collection_id != null;
  const hasRelease =
    card.availability === "available" && card.provider_id != null && card.external_id != null;

  const onClick = () => {
    if (isSeries) {
      navigate(`/collection/${card.collection_id}`);
    } else if (hasRelease) {
      setPreview(true);
    } else {
      // Lazy recommendation — no resolved release yet. Let the user pick one
      // from search (which previews each before download).
      navigate(`/search?q=${encodeURIComponent(card.title)}`);
    }
  };

  const kindLabel = card.kind === "tv" ? "Series" : "Movie";
  const typeLabel = card.is_anime ? `Anime · ${kindLabel}` : kindLabel;
  const subtitle = [typeLabel, card.year ? String(card.year) : null].filter(Boolean).join(" · ");

  // New-episode count for follows; otherwise a discreet seeder count when
  // known (1 seeder is fine — we never warn, only block 0 at grab time).
  let badge;
  if (card.new_count && card.new_count > 0) {
    badge = <Tag variant="accent">{card.new_count} new</Tag>;
  } else if (card.seeders && card.seeders > 0) {
    badge = <Tag variant="plain">{card.seeders}↑</Tag>;
  }

  return (
    <div className="group/card relative">
      <MediaCard
        onClick={onClick}
        title={card.title}
        subtitle={subtitle}
        posterUrl={card.poster_url ?? undefined}
        tmdbId={card.tmdb_id}
        kind={card.kind}
        badge={badge}
      />
      {/* Dismiss applies to recommendation candidates, not the user's own
          follows ("new episodes"). */}
      {!isSeries && (
        <button
          type="button"
          aria-label="Not interested"
          title="Not interested"
          onClick={() => dismiss.mutate()}
          disabled={dismiss.isPending}
          className="absolute left-1.5 top-1.5 z-10 grid size-6 place-items-center rounded-full bg-black/60 text-white/90 opacity-0 transition group-hover/card:opacity-100 hover:bg-black/80 focus-visible:opacity-100"
        >
          <X className="size-3.5" />
        </button>
      )}
      {hasRelease && (
        <PreviewDialog
          open={preview}
          onOpenChange={setPreview}
          providerId={card.provider_id}
          externalId={card.external_id}
          initialTitle={card.title}
          tmdbId={card.tmdb_id}
          seeders={card.seeders}
        />
      )}
    </div>
  );
}
