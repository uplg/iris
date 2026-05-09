import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { KeyRound, Link2, Trash2, Tv } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { ApiError, auth as authApi, devices as devicesApi, type DeviceView } from "@/lib/api";
import { useAuth } from "@/lib/auth";

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
    <div className="grid gap-6">
      <section>
        <h1 className="text-3xl font-semibold tracking-tight">Account</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Manage your identity, paired devices, and password.
        </p>
      </section>

      <IdentityCard />

      <DevicesCard />

      <Card className="max-w-md">
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
              <p className="text-sm text-emerald-300">
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
    if (auth.status === "authenticated") setName(auth.user.display_name);
  }, [auth.status === "authenticated" ? auth.user.display_name : ""]);

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
          <div className="flex size-14 shrink-0 items-center justify-center rounded-full bg-primary/10 text-lg font-semibold uppercase text-primary">
            {initials}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h2 className="truncate text-xl font-semibold leading-tight">{user.display_name}</h2>
              {user.is_admin && (
                <Badge
                  variant="outline"
                  className="border-fuchsia-400/50 text-[10px] uppercase text-fuchsia-300"
                >
                  admin
                </Badge>
              )}
            </div>
            <p className="truncate text-sm text-muted-foreground">{user.email}</p>
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
          <div className="flex min-h-[1.25rem] items-center gap-2 text-xs">
            {errMessage ? (
              <span className="text-destructive">{errMessage}</span>
            ) : justSaved && !dirty ? (
              <span className="text-emerald-300">Saved.</span>
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

function DevicesCard() {
  const qc = useQueryClient();
  const [params, setParams] = useSearchParams();
  const [code, setCode] = useState("");
  const [label, setLabel] = useState("");

  // Deep-link from a TV: GET /account?pair=ABCD-EFGH pre-fills the input.
  useEffect(() => {
    const fromUrl = params.get("pair");
    if (fromUrl && !code) {
      setCode(fromUrl.toUpperCase());
      params.delete("pair");
      setParams(params, { replace: true });
    }
  }, [params, code, setParams]);

  const list = useQuery({
    queryKey: ["devices"],
    queryFn: devicesApi.list,
  });

  const link = useMutation({
    mutationFn: ({ c, l }: { c: string; l: string }) => devicesApi.link(c, l.trim() || undefined),
    onSuccess: () => {
      setCode("");
      setLabel("");
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
                className="flex items-center justify-between gap-3 rounded-md border border-border bg-muted/30 px-3 py-2 text-sm"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Tv className="size-4 text-muted-foreground" />
                    <span className="truncate font-medium">
                      {d.label ?? d.kind ?? "Unnamed device"}
                    </span>
                    {d.kind && (
                      <Badge variant="outline" className="text-[10px] uppercase">
                        {d.kind}
                      </Badge>
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
