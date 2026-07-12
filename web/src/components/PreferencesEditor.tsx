import { Check } from "lucide-react";

import type { GenreOption, LanguageOption } from "@/lib/api";
import { cn } from "@/lib/utils";

type Props = {
  languages: string[];
  genres: number[];
  includeAnime: boolean;
  /** Server-driven selectable languages — never hardcoded client-side so
   *  adding a language is a backend-only change. */
  languageOptions: LanguageOption[];
  languagesLoading?: boolean;
  genreOptions: GenreOption[];
  genresLoading?: boolean;
  onToggleLanguage: (value: string) => void;
  onToggleGenre: (id: number) => void;
  onToggleAnime: () => void;
};

/**
 * Presentational editor for a user's recommendation preferences —
 * shared between the first-login OnboardingDialog and the Account page's
 * "Recommendations" card. Stateless: the parent owns the selection and
 * persistence. No timers / effects (CLAUDE.md web rule) — pure render.
 */
export function PreferencesEditor({
  languages,
  genres,
  includeAnime,
  languageOptions,
  languagesLoading,
  genreOptions,
  genresLoading,
  onToggleLanguage,
  onToggleGenre,
  onToggleAnime,
}: Props) {
  return (
    <div className="grid gap-6">
      <Section
        title="Languages"
        hint="What you'd rather watch in. We surface releases in these first."
      >
        {languagesLoading ? (
          <p className="text-[13px] text-muted-foreground">Loading languages…</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {languageOptions.map((l) => (
              <Chip
                key={l.value}
                selected={languages.includes(l.value)}
                onClick={() => onToggleLanguage(l.value)}
              >
                {l.label}
              </Chip>
            ))}
          </div>
        )}
      </Section>

      <Section
        title="Genres"
        hint="Pick a few you enjoy. Anime is its own category, distinct from Animation. Leave empty for a bit of everything."
      >
        <div className="flex flex-wrap gap-2">
          {/* Anime is a distinct category — NOT TMDB's "Animation" genre.
              It's backed by the AniList pipeline and driven by the separate
              include_anime preference, so it's always selectable here even
              before (or without) the TMDB genre list. */}
          <Chip selected={includeAnime} onClick={onToggleAnime} accent>
            Anime
          </Chip>
          {genresLoading ? (
            <span className="self-center text-[13px] text-muted-foreground">Loading genres…</span>
          ) : (
            genreOptions.map((g) => (
              <Chip key={g.id} selected={genres.includes(g.id)} onClick={() => onToggleGenre(g.id)}>
                {g.name}
              </Chip>
            ))
          )}
        </div>
      </Section>
    </div>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid gap-2.5">
      <div className="grid gap-0.5">
        <span className="text-sm font-medium text-foreground">{title}</span>
        <span className="text-[12.5px] text-fg-dim">{hint}</span>
      </div>
      {children}
    </div>
  );
}

function Chip({
  selected,
  onClick,
  accent,
  children,
}: {
  selected: boolean;
  onClick: () => void;
  /** Marks a special category (e.g. Anime) with a brand-tinted resting
   *  border so it reads as distinct from the plain TMDB genre chips. */
  accent?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[13px] font-medium transition",
        selected
          ? "border-primary bg-primary/15 text-foreground"
          : accent
            ? "border-primary/40 bg-elev text-foreground hover:border-primary"
            : "border-border bg-elev text-muted-foreground hover:text-foreground",
      )}
    >
      {selected && <Check className="size-3.5" />}
      {children}
    </button>
  );
}
