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
          Signed in as {auth.user.email}
          {auth.user.is_admin && " · admin"}
        </p>
      </section>

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
    mutationFn: ({ c, l }: { c: string; l: string }) =>
      devicesApi.link(c, l.trim() || undefined),
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
          Pair an Android TV (or any other Iris client) by entering the code it
          displays.
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
                    Linked {new Date(d.issued_at).toLocaleDateString()} ·
                    expires {new Date(d.expires_at).toLocaleDateString()}
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
