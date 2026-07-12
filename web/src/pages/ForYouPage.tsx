import { useQuery } from "@tanstack/react-query";

import { CatalogCardView } from "@/components/CatalogCardView";
import { Container } from "@/components/Container";
import { Shelf } from "@/components/Shelf";
import { me as meApi } from "@/lib/api";

/**
 * The "For You" page: the blended top picks plus organized sections
 * (per-genre, "because you watched X", new anime) — the expanded view of
 * the home shelf. Reachable from the nav and the home shelf's "see all".
 */
export function ForYouPage() {
  const q = useQuery({
    queryKey: ["for-you-page"],
    queryFn: meApi.forYouPage,
    staleTime: 60_000,
  });

  const shelves = q.data?.shelves ?? [];

  return (
    <Container>
      <div className="grid gap-2 py-2">
        <header className="grid gap-1.5 px-0.5">
          <span className="eyebrow">Discover</span>
          <h1 className="display" style={{ fontSize: "clamp(32px, 5vw, 52px)" }}>
            For You
          </h1>
        </header>

        {q.isLoading ? (
          <p className="px-0.5 text-sm text-muted-foreground">Loading…</p>
        ) : shelves.length === 0 ? (
          <p className="px-0.5 text-sm text-muted-foreground">
            Nothing to recommend yet. Set your preferences in your account and check back once the
            catalogue has refreshed.
          </p>
        ) : (
          <div className="lanes">
            {shelves.map((shelf) => (
              <Shelf key={shelf.key} title={shelf.title}>
                {shelf.items.map((card) => (
                  <CatalogCardView key={card.catalog_id} card={card} />
                ))}
              </Shelf>
            ))}
          </div>
        )}
      </div>
    </Container>
  );
}
