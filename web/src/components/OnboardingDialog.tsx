import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Sparkles } from "lucide-react";

import { PreferencesEditor } from "@/components/PreferencesEditor";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { discover, me as meApi, type Preferences } from "@/lib/api";

/**
 * First-login onboarding. Mounted on the Home page; opens itself when
 * the user's preferences exist but `onboarding_completed` is false.
 * Skippable — both actions persist `onboarding_completed: true` so it
 * never reappears. No timers (CLAUDE.md web rule): visibility is driven
 * entirely by the preferences query + a one-shot dismissed flag.
 */
export function OnboardingDialog() {
  const qc = useQueryClient();
  const prefsQ = useQuery({
    queryKey: ["preferences"],
    queryFn: meApi.preferences,
    staleTime: 5 * 60_000,
  });
  const genresQ = useQuery({
    queryKey: ["genres"],
    queryFn: discover.genres,
    staleTime: 24 * 60 * 60_000,
  });
  const languagesQ = useQuery({
    queryKey: ["languages"],
    queryFn: discover.languages,
    staleTime: 24 * 60 * 60_000,
  });

  const [languages, setLanguages] = useState<string[]>([]);
  const [genres, setGenres] = useState<number[]>([]);
  const [includeAnime, setIncludeAnime] = useState(false);
  const [seeded, setSeeded] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  // Seed local selections from the server prefs the first time they
  // arrive (idempotent one-shot guard — the React-blessed "derive state
  // from props" pattern, no effect/timer needed).
  if (!seeded && prefsQ.data) {
    setSeeded(true);
    setLanguages(prefsQ.data.languages);
    setGenres(prefsQ.data.genres);
    setIncludeAnime(prefsQ.data.include_anime);
  }

  const save = useMutation({
    mutationFn: (body: Preferences) => meApi.savePreferences(body),
    onSuccess: (data) => {
      // Write straight into the cache so `open` recomputes to false
      // without waiting on a refetch.
      qc.setQueryData(["preferences"], data);
    },
  });

  const open = !dismissed && prefsQ.data != null && !prefsQ.data.onboarding_completed;

  // `keep` distinguishes "Save preferences" (persist selections) from
  // "Skip for now" (clear them — cold-start fallback kicks in). Both
  // mark onboarding complete.
  const finish = (keep: boolean) => {
    setDismissed(true);
    save.mutate({
      languages: keep ? languages : [],
      genres: keep ? genres : [],
      include_anime: keep ? includeAnime : false,
      onboarding_completed: true,
    });
  };

  const toggleLanguage = (value: string) =>
    setLanguages((cur) => (cur.includes(value) ? cur.filter((x) => x !== value) : [...cur, value]));
  const toggleGenre = (id: number) =>
    setGenres((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        // Escape / overlay / close button → treat as skip.
        if (!o) finish(false);
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-4.5 text-primary" />
            Personalize your home
          </DialogTitle>
          <DialogDescription>
            Tell us what you're into and we'll tune your recommendations. You can change this
            anytime in your account.
          </DialogDescription>
        </DialogHeader>

        <PreferencesEditor
          languages={languages}
          genres={genres}
          includeAnime={includeAnime}
          languageOptions={languagesQ.data?.languages ?? []}
          languagesLoading={languagesQ.isLoading}
          genreOptions={genresQ.data?.genres ?? []}
          genresLoading={genresQ.isLoading}
          onToggleLanguage={toggleLanguage}
          onToggleGenre={toggleGenre}
          onToggleAnime={() => setIncludeAnime((v) => !v)}
        />

        <DialogFooter>
          <Button variant="ghost" onClick={() => finish(false)} disabled={save.isPending}>
            Skip for now
          </Button>
          <Button onClick={() => finish(true)} disabled={save.isPending}>
            {save.isPending ? "Saving…" : "Save preferences"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
