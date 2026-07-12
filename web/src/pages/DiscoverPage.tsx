import { useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";

import { CatalogCardView } from "@/components/CatalogCardView";
import { Container } from "@/components/Container";
import { Shelf } from "@/components/Shelf";
import { me as meApi, type MediaKind, type MoodTile } from "@/lib/api";
import { cn } from "@/lib/utils";

const discoverApi = getRouteApi("/auth/shell/discover");

type Kind = MediaKind;

/**
 * The single "Discover" destination — both halves of the reco system
 * as tabs ("For You" + "Tonight"), mirroring the TV. All state lives
 * in the URL (`?view=&mood=&kind=`) so tabs, boards and mood results
 * stay shareable and Back-friendly; `/for-you` and `/moods` redirect
 * here.
 */
export function DiscoverPage() {
  const { view, mood, kind } = discoverApi.useSearch();
  const navigate = discoverApi.useNavigate();
  const k: Kind = kind ?? "movie";
  const tonight = view === "tonight";

  return (
    <Container>
      <div className="grid gap-5 py-2">
        <header className="flex flex-wrap items-center gap-4 px-0.5">
          <span className="eyebrow">Discover</span>
          <div className="inline-flex rounded-full border border-border bg-elev p-0.5 text-sm">
            <button
              type="button"
              onClick={() => navigate({ search: {}, replace: true })}
              className={cn(
                "rounded-full px-3.5 py-1 transition",
                !tonight ? "bg-primary text-white" : "text-fg-dim hover:opacity-80",
              )}
            >
              For You
            </button>
            <button
              type="button"
              onClick={() => navigate({ search: { view: "tonight", kind: k }, replace: true })}
              className={cn(
                "rounded-full px-3.5 py-1 transition",
                tonight ? "bg-primary text-white" : "text-fg-dim hover:opacity-80",
              )}
            >
              Tonight
            </button>
          </div>
        </header>

        {!tonight ? (
          <ForYouSection />
        ) : mood ? (
          <MoodResultsView moodId={mood} kind={k} />
        ) : (
          <MoodBoardView kind={k} />
        )}
      </div>
    </Container>
  );
}

// "For You" tab — the expanded view of the home shelf.

function ForYouSection() {
  const q = useQuery({
    queryKey: ["for-you-page"],
    queryFn: meApi.forYouPage,
    staleTime: 60_000,
  });

  const shelves = q.data?.shelves ?? [];

  if (q.isLoading) {
    return <p className="px-0.5 text-sm text-muted-foreground">Loading…</p>;
  }
  if (shelves.length === 0) {
    return (
      <p className="px-0.5 text-sm text-muted-foreground">
        Nothing to recommend yet. Set your preferences in your account and check back once the
        catalogue has refreshed.
      </p>
    );
  }
  return (
    <div className="lanes">
      {shelves.map((shelf) => (
        <Shelf key={shelf.key} title={shelf.title}>
          {shelf.items.map((card) => (
            <CatalogCardView key={card.catalog_id} card={card} />
          ))}
        </Shelf>
      ))}
    </div>
  );
}

// "Tonight" tab — the mood board + per-mood results.

function KindToggle({ kind, onChange }: { kind: Kind; onChange: (k: Kind) => void }) {
  return (
    <div className="inline-flex rounded-full border border-border bg-elev p-0.5 text-sm">
      {(["movie", "tv"] as const).map((option) => (
        <button
          key={option}
          type="button"
          onClick={() => onChange(option)}
          className={cn(
            "rounded-full px-3.5 py-1 transition",
            kind === option ? "bg-primary text-white" : "text-fg-dim hover:opacity-80",
          )}
        >
          {option === "movie" ? "Films" : "Series"}
        </button>
      ))}
    </div>
  );
}

function MoodTileButton({ tile, onClick }: { tile: MoodTile; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group relative aspect-[16/10] overflow-hidden rounded-2xl border border-border bg-elev text-left transition hover:border-border-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {tile.backdrop_url ? (
        <img
          src={tile.backdrop_url}
          alt=""
          loading="lazy"
          className="absolute inset-0 h-full w-full object-cover opacity-65 transition duration-300 group-hover:scale-[1.05] group-hover:opacity-80"
        />
      ) : (
        <div className="absolute inset-0 bg-gradient-to-br from-primary/25 to-transparent" />
      )}
      {/* Legibility scrim under the label. */}
      <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/15 to-transparent" />
      <span className="absolute inset-x-0 bottom-0 line-clamp-2 p-3 text-base font-semibold leading-tight text-white drop-shadow-sm sm:text-lg">
        {tile.label}
      </span>
    </button>
  );
}

function MoodBoardView({ kind }: { kind: Kind }) {
  const navigate = discoverApi.useNavigate();
  const q = useQuery({ queryKey: ["mood-board", kind], queryFn: () => meApi.moodBoard(kind) });
  const moods = q.data?.moods ?? [];

  return (
    <div className="grid gap-5">
      <header className="flex flex-wrap items-end justify-between gap-3 px-0.5">
        <div className="grid gap-1">
          <h1 className="text-2xl font-semibold tracking-tight">What are you in the mood for?</h1>
          <p className="text-sm text-fg-dim">Tonight&apos;s picks, tuned for you.</p>
        </div>
        <KindToggle
          kind={kind}
          onChange={(next) => navigate({ search: { view: "tonight", kind: next }, replace: true })}
        />
      </header>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
        {moods.map((tile) => (
          <MoodTileButton
            key={tile.id}
            tile={tile}
            onClick={() => navigate({ search: { view: "tonight", mood: tile.id, kind } })}
          />
        ))}
      </div>
    </div>
  );
}

function MoodResultsView({ moodId, kind }: { moodId: string; kind: Kind }) {
  const navigate = discoverApi.useNavigate();
  // The board query is cached from the grid; reuse it to label the header
  // without an extra round-trip (or fetch it on a deep link).
  const board = useQuery({ queryKey: ["mood-board", kind], queryFn: () => meApi.moodBoard(kind) });
  const label = board.data?.moods.find((m) => m.id === moodId)?.label ?? "Mood";
  const q = useQuery({
    queryKey: ["mood-results", moodId, kind],
    queryFn: () => meApi.moodResults(moodId, kind),
  });
  const items = q.data?.items ?? [];

  return (
    <div className="grid gap-4">
      <header className="flex flex-wrap items-center justify-between gap-3 px-0.5">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => navigate({ search: { view: "tonight", kind } })}
            aria-label="Back to moods"
            className="grid size-9 place-items-center rounded-full border border-border text-fg-dim transition hover:opacity-80"
          >
            <ArrowLeft className="size-4" />
          </button>
          <div className="grid gap-0.5">
            <h1 className="text-2xl font-semibold tracking-tight">{label}</h1>
            <p className="text-sm text-fg-dim">{kind === "movie" ? "Films" : "Series"}</p>
          </div>
        </div>
        <KindToggle
          kind={kind}
          onChange={(next) => navigate({ search: { view: "tonight", mood: moodId, kind: next } })}
        />
      </header>
      {q.isPending ? (
        <p className="px-0.5 text-sm text-fg-dim">Finding something good…</p>
      ) : items.length === 0 ? (
        <p className="px-0.5 text-sm text-fg-dim">Nothing grabbable for this mood right now.</p>
      ) : (
        <div className="flex flex-wrap gap-4">
          {items.map((card) => (
            <CatalogCardView key={card.catalog_id} card={card} />
          ))}
        </div>
      )}
    </div>
  );
}
