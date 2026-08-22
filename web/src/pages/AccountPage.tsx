import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { getRouteApi } from "@tanstack/react-router";
import { KeyRound, Link2, LogOut, Trash2, Tv } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Container } from "@/components/Container";
import { PreferencesEditor } from "@/components/PreferencesEditor";
import { AccentSwatches } from "@/components/TweaksDrawer";
import { Tag } from "@/components/Tag";
import {
  ApiError,
  auth as authApi,
  devices as devicesApi,
  discover,
  me as meApi,
  type DeviceView,
  type Preferences,
} from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";

export function AccountPage() {
  const auth = useAuth();
  const [oldPwd, setOldPwd] = useState("");
  const [newPwd, setNewPwd] = useState("");
  const [confirm, setConfirm] = useState("");
  const [success, setSuccess] = useState(false);

  const change = useMutation({
    mutationFn: ({ oldP, newP }: { oldP: string; newP: string }) =>
      authApi.changePassword(oldP, newP),
    onSuccess: () => {
      setOldPwd("");
      setNewPwd("");
      setConfirm("");
      setSuccess(true);
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSuccess(false);
    if (newPwd.length < 8) {
      change.reset();
      return;
    }
    if (newPwd !== confirm) return;
    change.mutate({ oldP: oldPwd, newP: newPwd });
  };

  if (auth.status !== "authenticated") return null;

  const errMessage = change.error
    ? change.error instanceof ApiError
      ? change.error.message
      : String(change.error)
    : null;
  const localErr =
    newPwd.length > 0 && newPwd.length < 8
      ? "Password must be at least 8 characters."
      : confirm.length > 0 && newPwd !== confirm
        ? "Passwords don't match."
        : null;

  return (
    <Container narrow>
      <div className="grid gap-8">
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div className="grid gap-1.5">
            <span className="eyebrow">Profile</span>
            <h1 className="display" style={{ fontSize: "clamp(36px, 5vw, 56px)" }}>
              Account
            </h1>
          </div>
          <Button variant="outline" onClick={() => void auth.logout()}>
            <LogOut className="size-4" /> Sign out
          </Button>
        </header>

        <IdentityCard />

        <PreferencesCard />

        <RecommendationsCard />

        <DevicesCard />

        <Card>
          <CardHeader>
            <CardTitle>Change password</CardTitle>
            <CardDescription>Other sessions will be signed out automatically.</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={onSubmit} className="grid gap-4">
              <div className="grid gap-2">
                <Label htmlFor="oldPwd">Current password</Label>
                <Input
                  id="oldPwd"
                  type="password"
                  autoComplete="current-password"
                  required
                  value={oldPwd}
                  onChange={(e) => setOldPwd(e.target.value)}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="newPwd">New password</Label>
                <Input
                  id="newPwd"
                  type="password"
                  autoComplete="new-password"
                  minLength={8}
                  required
                  value={newPwd}
                  onChange={(e) => setNewPwd(e.target.value)}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="confirm">Confirm new password</Label>
                <Input
                  id="confirm"
                  type="password"
                  autoComplete="new-password"
                  minLength={8}
                  required
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                />
              </div>
              {(localErr || errMessage) && (
                <p className="text-sm text-destructive">{localErr ?? errMessage}</p>
              )}
              {success && (
                <p className="text-sm text-success">
                  Password updated. Other devices have been signed out.
                </p>
              )}
              <Button
                type="submit"
                disabled={
                  change.isPending || !oldPwd || !newPwd || newPwd !== confirm || newPwd.length < 8
                }
              >
                <KeyRound className="size-4" />
                {change.isPending ? "Updating…" : "Update password"}
              </Button>
            </form>
          </CardContent>
        </Card>
      </div>
    </Container>
  );
}

function PreferencesCard() {
  const { resolved, setTheme, accent, setAccent, density, setDensity } = useTheme();
  return (
    <section className="grid gap-3">
      <div className="grid gap-1">
        <span className="eyebrow">Display</span>
        <h2 className="heading-3">Preferences</h2>
      </div>
      <Card className="overflow-hidden">
        <CardContent className="grid gap-0 p-0">
          <SettingsRow label="Theme" description="Choose between dark and light." first>
            <div className="inline-flex gap-1.5 rounded-[10px] border border-border bg-elev p-1">
              {(["dark", "light"] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setTheme(t)}
                  className={cn(
                    "rounded-md px-3.5 py-1.5 text-[13px] font-medium capitalize transition",
                    resolved === t
                      ? "border border-border bg-elev-2 text-foreground"
                      : "border border-transparent text-muted-foreground",
                  )}
                >
                  {t}
                </button>
              ))}
            </div>
          </SettingsRow>
          <SettingsRow
            label="Accent color"
            description="Used for primary actions, focus rings, and highlights."
          >
            <AccentSwatches value={accent} onChange={setAccent} />
          </SettingsRow>
          <SettingsRow label="Density" description="Spacing rhythm across pages.">
            <div className="inline-flex gap-1.5 rounded-[10px] border border-border bg-elev p-1">
              {(["comfortable", "compact"] as const).map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => setDensity(d)}
                  className={cn(
                    "rounded-md px-3.5 py-1.5 text-[13px] font-medium capitalize transition",
                    density === d
                      ? "border border-border bg-elev-2 text-foreground"
                      : "border border-transparent text-muted-foreground",
                  )}
                >
                  {d}
                </button>
              ))}
            </div>
          </SettingsRow>
        </CardContent>
      </Card>
    </section>
  );
}

function RecommendationsCard() {
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
  const [justSaved, setJustSaved] = useState(false);

  // One-shot seed from server prefs (derive-state-from-data pattern,
  // no effect/timer).
  if (!seeded && prefsQ.data) {
    setSeeded(true);
    setLanguages(prefsQ.data.languages);
    setGenres(prefsQ.data.genres);
    setIncludeAnime(prefsQ.data.include_anime);
  }

  const save = useMutation({
    mutationFn: (body: Preferences) => meApi.savePreferences(body),
    onSuccess: (data) => {
      qc.setQueryData(["preferences"], data);
      setJustSaved(true);
    },
  });

  const dirty = prefsQ.data
    ? !arraysEqual(languages, prefsQ.data.languages) ||
      !arraysEqual(genres, prefsQ.data.genres) ||
      includeAnime !== prefsQ.data.include_anime
    : false;

  const touch = () => setJustSaved(false);
  const toggleLanguage = (value: string) => {
    touch();
    setLanguages((cur) => (cur.includes(value) ? cur.filter((x) => x !== value) : [...cur, value]));
  };
  const toggleGenre = (id: number) => {
    touch();
    setGenres((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));
  };
  const toggleAnime = () => {
    touch();
    setIncludeAnime((v) => !v);
  };

  return (
    <section className="grid gap-3">
      <div className="grid gap-1">
        <span className="eyebrow">For You</span>
        <h2 className="heading-3">Recommendations</h2>
      </div>
      <Card>
        <CardContent className="grid gap-6 p-6">
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
            onToggleAnime={toggleAnime}
          />
          <div className="flex items-center gap-3">
            <Button
              onClick={() =>
                save.mutate({
                  languages,
                  genres,
                  include_anime: includeAnime,
                  onboarding_completed: prefsQ.data?.onboarding_completed ?? true,
                })
              }
              disabled={!dirty || save.isPending}
            >
              {save.isPending ? "Saving…" : "Save"}
            </Button>
            {justSaved && !dirty && <span className="text-sm text-success">Saved.</span>}
          </div>
        </CardContent>
      </Card>
    </section>
  );
}

function arraysEqual<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

function SettingsRow({
  label,
  description,
  children,
  first,
}: {
  label: string;
  description: string;
  children: React.ReactNode;
  first?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-center justify-between gap-4 p-4",
        !first && "border-t border-border",
      )}
    >
      <div className="grid min-w-50 flex-1 gap-0.5">
        <span className="text-sm font-medium text-foreground">{label}</span>
        <span className="text-[12.5px] text-fg-dim">{description}</span>
      </div>
      {children}
    </div>
  );
}

function IdentityCard() {
  const auth = useAuth();
  const qc = useQueryClient();
  const currentName = auth.status === "authenticated" ? auth.user.display_name : "";
  const [name, setName] = useState(currentName);
  const [justSaved, setJustSaved] = useState(false);

  // Re-sync local input if the auth user changes (e.g. /me refresh).
  useEffect(() => {
    setName(currentName);
  }, [currentName]);

  const save = useMutation({
    mutationFn: (n: string) => authApi.changeDisplayName(n),
    onSuccess: () => {
      setJustSaved(true);
      void qc.invalidateQueries({ queryKey: ["me"] });
    },
  });

  if (auth.status !== "authenticated") return null;
  const user = auth.user;
  const trimmed = name.trim();
  const dirty = trimmed.length > 0 && trimmed !== user.display_name;
  const errMessage = save.error
    ? save.error instanceof ApiError
      ? save.error.message
      : String(save.error)
    : null;
  const initials = initialsFor(user.display_name || user.email);
  const emailDefault = emailLocalDefault(user.email);

  return (
    <Card className="max-w-2xl">
      <CardContent className="grid gap-6 p-6">
        <div className="flex items-center gap-4">
          <div
            className="grid size-16 shrink-0 place-items-center rounded-[20px] font-display text-2xl font-semibold uppercase text-primary-foreground"
            style={{
              background: "linear-gradient(135deg, var(--brand-3), var(--brand), var(--brand-2))",
            }}
          >
            {initials}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h2 className="heading-3 truncate text-xl">{user.display_name}</h2>
              {user.is_admin && (
                <Tag variant="accent" upper>
                  Admin
                </Tag>
              )}
            </div>
            <p className="truncate font-mono text-[13px] text-muted-foreground">{user.email}</p>
          </div>
        </div>

        <div className="border-t border-border" />

        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            setJustSaved(false);
            if (!dirty) return;
            save.mutate(trimmed);
          }}
        >
          <div className="grid gap-1.5">
            <Label htmlFor="displayName" className="text-xs uppercase tracking-wide">
              Display name
            </Label>
            <p className="text-xs text-muted-foreground">
              Shown in attributions like "added by". Your email stays private.
            </p>
          </div>
          <div className="flex flex-col gap-2 sm:flex-row sm:items-start">
            <Input
              id="displayName"
              maxLength={64}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setJustSaved(false);
              }}
              className="sm:flex-1"
            />
            <Button type="submit" disabled={!dirty || save.isPending} className="sm:w-auto">
              {save.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
          <div className="flex min-h-5 items-center gap-2 text-xs">
            {errMessage ? (
              <span className="text-destructive">{errMessage}</span>
            ) : justSaved && !dirty ? (
              <span className="text-success">Saved.</span>
            ) : emailDefault && trimmed !== emailDefault ? (
              <span className="text-muted-foreground">
                Default from email:{" "}
                <button
                  type="button"
                  className="font-medium text-foreground hover:underline"
                  onClick={() => {
                    setName(emailDefault);
                    setJustSaved(false);
                  }}
                >
                  {emailDefault}
                </button>
              </span>
            ) : null}
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

function initialsFor(source: string): string {
  const cleaned =
    source
      .split("@")[0]
      ?.replace(/[^a-zA-Z0-9]+/g, " ")
      .trim() ?? "";
  if (!cleaned) return "?";
  const parts = cleaned.split(/\s+/);
  if (parts.length >= 2) return (parts[0]![0]! + parts[1]![0]!).toUpperCase();
  return cleaned.slice(0, 2).toUpperCase();
}

function emailLocalDefault(email: string): string {
  return email.split("@")[0]?.split(".")[0] ?? "";
}

const accountRoute = getRouteApi("/auth/shell/account");

function DevicesCard() {
  const qc = useQueryClient();
  const { pair } = accountRoute.useSearch();
  const navigate = accountRoute.useNavigate();
  const [code, setCode] = useState("");
  const [label, setLabel] = useState("");

  // Deep-link from a TV: GET /account?pair=ABCD-EFGH pre-fills the input,
  // then strips the param so a refresh doesn't re-fill it.
  useEffect(() => {
    if (pair && !code) {
      setCode(pair.toUpperCase());
      navigate({ search: (prev) => ({ ...prev, pair: undefined }), replace: true });
    }
  }, [pair, code, navigate]);

  // After a successful link, the device only appears in the list once the TV
  // polls /auth/device/poll and a refresh_token row is created. Poll briefly
  // so the new device shows up without a manual refresh.
  const [awaitingPair, setAwaitingPair] = useState(false);
  const expectedCountRef = useRef(0);

  const list = useQuery({
    queryKey: ["devices"],
    queryFn: devicesApi.list,
    refetchInterval: awaitingPair ? 1500 : false,
  });

  useEffect(() => {
    if (awaitingPair && list.data && list.data.length > expectedCountRef.current) {
      setAwaitingPair(false);
    }
  }, [awaitingPair, list.data]);

  useEffect(() => {
    if (!awaitingPair) return;
    const t = setTimeout(() => setAwaitingPair(false), 30_000);
    return () => clearTimeout(t);
  }, [awaitingPair]);

  const link = useMutation({
    mutationFn: ({ c, l }: { c: string; l: string }) => devicesApi.link(c, l.trim() || undefined),
    onSuccess: () => {
      setCode("");
      setLabel("");
      expectedCountRef.current = list.data?.length ?? 0;
      setAwaitingPair(true);
      void qc.invalidateQueries({ queryKey: ["devices"] });
    },
  });

  const revoke = useMutation({
    mutationFn: (jti: string) => devicesApi.revoke(jti),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["devices"] });
    },
  });

  const onLink = (e: FormEvent) => {
    e.preventDefault();
    link.mutate({ c: code.trim().toUpperCase(), l: label });
  };

  const error = link.error
    ? link.error instanceof ApiError
      ? link.error.message
      : String(link.error)
    : null;

  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle>Devices</CardTitle>
        <CardDescription>
          Pair an Android TV (or any other Iris client) by entering the code it displays.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <form onSubmit={onLink} className="grid gap-3 sm:grid-cols-[1fr_1fr_auto] sm:items-end">
          <div className="grid gap-2">
            <Label htmlFor="pair-code">Pairing code</Label>
            <Input
              id="pair-code"
              value={code}
              onChange={(e) => setCode(e.target.value.toUpperCase())}
              placeholder="WX7K-ABCD"
              autoComplete="off"
              required
              className="font-mono uppercase tracking-wider"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="pair-label">Device name</Label>
            <Input
              id="pair-label"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="Living room TV"
            />
          </div>
          <Button type="submit" disabled={link.isPending || code.trim().length < 4}>
            <Link2 className="size-4" />
            {link.isPending ? "Linking…" : "Link"}
          </Button>
        </form>
        {error && <p className="text-sm text-destructive">{error}</p>}

        {list.isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : list.data && list.data.length > 0 ? (
          <ul className="grid gap-2">
            {list.data.map((d: DeviceView) => (
              <li
                key={d.jti}
                className="flex items-center justify-between gap-3 rounded-md border border-border bg-elev px-3 py-2 text-sm"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Tv className="size-4 text-muted-foreground" />
                    <span className="truncate font-medium">
                      {d.label ?? d.kind ?? "Unnamed device"}
                    </span>
                    {d.kind && (
                      <Tag variant="plain" upper>
                        {d.kind}
                      </Tag>
                    )}
                  </div>
                  <span className="text-[11px] text-muted-foreground">
                    Linked {new Date(d.issued_at).toLocaleDateString()} · expires{" "}
                    {new Date(d.expires_at).toLocaleDateString()}
                  </span>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => revoke.mutate(d.jti)}
                  disabled={revoke.isPending}
                  title="Revoke"
                >
                  <Trash2 className="size-4" />
                  <span className="sr-only">Revoke</span>
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">No paired devices yet.</p>
        )}
      </CardContent>
    </Card>
  );
}
