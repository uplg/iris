import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";

import { MediaCard } from "@/components/MediaCard";
import { Tag } from "@/components/Tag";
import { me as meApi, type CatalogCard } from "@/lib/api";

/**
 * A "For You" recommendation card. Routes to the collection for a
 * followed series with new episodes, else searches the trackers for the
 * title. Recommendation candidates (not the user's own follows) get a
 * "not interested" dismiss button on hover.
 */
export function CatalogCardView({ card }: { card: CatalogCard }) {
  const qc = useQueryClient();
  const dismiss = useMutation({
    mutationFn: () => meApi.dismissForYou(card.catalog_id),
    onSuccess: () => {
      // Invalidate both surfaces that render For-You cards: the home shelf
      // (`["for-you"]`) and the organized page (`["for-you-page"]`).
      void qc.invalidateQueries({ queryKey: ["for-you"] });
      void qc.invalidateQueries({ queryKey: ["for-you-page"] });
    },
  });

  const href = card.collection_id
    ? `/collection/${card.collection_id}`
    : `/search?q=${encodeURIComponent(card.title)}`;
  // Always say what it is — Movie / Series, prefixed with "Anime" for the
  // anime catalogue (which mixes movies and series). Solves "can't tell a
  // series from a film" on the blended shelves.
  const kindLabel = card.kind === "tv" ? "Series" : "Movie";
  const typeLabel = card.is_anime ? `Anime · ${kindLabel}` : kindLabel;
  const subtitle = [typeLabel, card.year ? String(card.year) : null].filter(Boolean).join(" · ");
  const badge =
    card.new_count && card.new_count > 0 ? <Tag variant="accent">{card.new_count} new</Tag> : undefined;

  return (
    <div className="group/card relative">
      <MediaCard
        href={href}
        title={card.title}
        subtitle={subtitle}
        posterUrl={card.poster_url ?? undefined}
        tmdbId={card.tmdb_id}
        kind={card.kind}
        badge={badge}
      />
      {/* Dismiss applies to recommendation candidates, not the user's own
          follows ("new episodes"). */}
      {!card.collection_id && (
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
    </div>
  );
}
