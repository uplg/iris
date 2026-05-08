import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { KeyRound, Plus, RotateCcw, ShieldCheck, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  admin,
  type CreatedInvitation,
  type GcReport,
  type Invitation,
  type UserView,
} from "@/lib/api";
import { formatSize } from "@/lib/format";

function status(inv: Invitation) {
  if (inv.consumed_at) return "consumed";
  if (new Date(inv.expires_at) < new Date()) return "expired";
  return "active";
}

export function AdminPage() {
  const qc = useQueryClient();
  const [lastCreated, setLastCreated] = useState<CreatedInvitation | null>(null);
  const [lastGc, setLastGc] = useState<GcReport | null>(null);

  const invitations = useQuery({
    queryKey: ["admin", "invitations"],
    queryFn: admin.listInvitations,
  });

  const storage = useQuery({
    queryKey: ["admin", "storage"],
    queryFn: admin.storage,
    refetchInterval: 10_000,
  });

  const gc = useMutation({
    mutationFn: admin.triggerGc,
    onSuccess: (report) => {
      setLastGc(report);
      void qc.invalidateQueries({ queryKey: ["admin", "storage"] });
      void qc.invalidateQueries({ queryKey: ["torrents"] });
    },
  });

  const create = useMutation({
    mutationFn: () => admin.createInvitation(),
    onSuccess: (created) => {
      setLastCreated(created);
      void qc.invalidateQueries({ queryKey: ["admin", "invitations"] });
    },
  });

  const revoke = useMutation({
    mutationFn: (id: string) => admin.revokeInvitation(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["admin", "invitations"] });
    },
  });

  return (
    <div className="grid gap-8">
      <section>
        <h1 className="text-3xl font-semibold tracking-tight">Admin</h1>
        <p className="mt-1 text-muted-foreground">Invitations and account management.</p>
      </section>

      <Card>
        <CardHeader>
          <CardTitle>Storage</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          {storage.data ? (
            <StorageView data={storage.data} />
          ) : storage.isLoading ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : null}
          <div className="flex items-center gap-3">
            <Button onClick={() => gc.mutate()} disabled={gc.isPending}>
              <RotateCcw className={`size-4 ${gc.isPending ? "animate-spin" : ""}`} />
              {gc.isPending ? "Running GC…" : "Run GC now"}
            </Button>
            {gc.error && (
              <span className="text-sm text-destructive">
                {gc.error instanceof Error ? gc.error.message : "failed"}
              </span>
            )}
          </div>
          {lastGc && <GcReportView report={lastGc} />}
        </CardContent>
      </Card>

      <UsersCard />

      <Card>
        <CardHeader>
          <CardTitle>Invitations</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-6">
          <div className="flex items-center gap-3">
            <Button onClick={() => create.mutate()} disabled={create.isPending}>
              <Plus className="size-4" />
              {create.isPending ? "Generating…" : "Generate invitation"}
            </Button>
            {create.error && (
              <span className="text-sm text-destructive">{create.error.message}</span>
            )}
          </div>

          {lastCreated && (
            <div className="rounded-md border border-border bg-muted/30 p-4">
              <p className="text-sm text-muted-foreground">
                Token shown ONCE. Share it with the invitee.
              </p>
              <code className="mt-2 block break-all rounded bg-background px-3 py-2 text-sm">
                {lastCreated.token}
              </code>
              <p className="mt-2 text-xs text-muted-foreground">
                Or share this link:{" "}
                <code className="text-xs">
                  {window.location.origin}/register?token={lastCreated.token}
                </code>
              </p>
            </div>
          )}

          {invitations.isLoading ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : invitations.data?.length ? (
            <div className="overflow-hidden rounded-md border border-border">
              <table className="w-full text-sm">
                <thead className="bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
                  <tr>
                    <th className="px-4 py-2 text-left">Created</th>
                    <th className="px-4 py-2 text-left">Expires</th>
                    <th className="px-4 py-2 text-left">Status</th>
                    <th className="px-4 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {invitations.data.map((inv) => {
                    const s = status(inv);
                    return (
                      <tr key={inv.id} className="border-t border-border">
                        <td className="px-4 py-2">{new Date(inv.created_at).toLocaleString()}</td>
                        <td className="px-4 py-2">{new Date(inv.expires_at).toLocaleString()}</td>
                        <td className="px-4 py-2">{s}</td>
                        <td className="px-4 py-2 text-right">
                          {s === "active" && (
                            <Button
                              size="sm"
                              variant="outline"
                              onClick={() => revoke.mutate(inv.id)}
                              disabled={revoke.isPending}
                              title="Revoke"
                            >
                              <Trash2 className="size-3.5" />
                              <span className="sr-only">Revoke</span>
                            </Button>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">No invitations yet.</p>
          )}
        </CardContent>
      </Card>

      <HlsCacheCard />
    </div>
  );
}

function HlsCacheCard() {
  const qc = useQueryClient();
  const jobs = useQuery({
    queryKey: ["admin", "hls"],
    queryFn: admin.listHls,
    refetchInterval: 10_000,
  });
  const wipe = useMutation({
    mutationFn: (key: string) => admin.wipeHls(key),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "hls"] }),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>HLS cache</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-3">
        <p className="text-sm text-muted-foreground">
          Pre-segmentation jobs. Wipe when a job is stuck failing or produced
          a truncated playlist (last fail timestamp set, or video segments
          way under the source duration). Re-segmentation triggers
          automatically on the next play attempt.
        </p>
        {jobs.data && jobs.data.length === 0 && (
          <p className="text-sm text-muted-foreground">No HLS dirs on disk.</p>
        )}
        {jobs.data && jobs.data.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="text-left text-xs text-muted-foreground">
                <tr>
                  <th className="px-2 py-1">Title</th>
                  <th className="px-2 py-1">State</th>
                  <th className="px-2 py-1 text-right">Segments</th>
                  <th className="px-2 py-1 text-right">Disk</th>
                  <th className="px-2 py-1">Last fail</th>
                  <th className="px-2 py-1"></th>
                </tr>
              </thead>
              <tbody>
                {jobs.data.map((j) => {
                  const broken = j.last_failed_at != null;
                  const stale =
                    j.expected_duration_secs != null &&
                    j.video_segments > 0 &&
                    j.video_segments < (j.expected_duration_secs / 6) * 0.5;
                  const stateLabel = j.running
                    ? "running"
                    : broken
                      ? "failed"
                      : stale
                        ? "truncated"
                        : j.done
                          ? "ready"
                          : "partial";
                  const stateClass =
                    stateLabel === "ready"
                      ? "bg-emerald-500/10 text-emerald-300"
                      : stateLabel === "running"
                        ? "bg-sky-500/10 text-sky-300"
                        : stateLabel === "failed" || stateLabel === "truncated"
                          ? "bg-rose-500/10 text-rose-300"
                          : "bg-zinc-500/10 text-zinc-300";
                  return (
                    <tr key={j.key} className="border-t border-border/50">
                      <td className="max-w-md truncate px-2 py-1.5" title={j.torrent_name ?? j.key}>
                        {j.torrent_name ?? <span className="font-mono text-xs">{j.key}</span>}
                        {j.file_idx != null && (
                          <span className="ml-1 text-xs text-muted-foreground">
                            (file #{j.file_idx})
                          </span>
                        )}
                      </td>
                      <td className="px-2 py-1.5">
                        <Badge variant="outline" className={`text-[10px] uppercase ${stateClass}`}>
                          {stateLabel}
                        </Badge>
                      </td>
                      <td className="px-2 py-1.5 text-right tabular-nums">
                        {j.video_segments}
                      </td>
                      <td className="px-2 py-1.5 text-right tabular-nums">
                        {formatSize(j.disk_bytes)}
                      </td>
                      <td className="px-2 py-1.5 text-xs text-muted-foreground">
                        {j.last_failed_at
                          ? new Date(j.last_failed_at * 1000).toLocaleString()
                          : "—"}
                      </td>
                      <td className="px-2 py-1.5 text-right">
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => wipe.mutate(j.key)}
                          disabled={wipe.isPending}
                          title="Wipe this HLS cache directory"
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function StorageView({
  data,
}: {
  data: ReturnType<
    typeof useQuery<{
      used_bytes: number;
      max_storage_bytes: number;
      threshold_bytes: number;
      target_bytes: number;
      threshold_pct: number;
      target_pct: number;
      torrent_count: number;
    }>
  >["data"] &
    object;
}) {
  const usedPct =
    data.max_storage_bytes > 0
      ? Math.min(100, (data.used_bytes / data.max_storage_bytes) * 100)
      : 0;
  const overThreshold = data.used_bytes >= data.threshold_bytes;
  return (
    <div className="grid gap-2">
      <div className="flex items-baseline justify-between text-sm">
        <span className="text-muted-foreground">
          {formatSize(data.used_bytes)} / {formatSize(data.max_storage_bytes)} ·{" "}
          {data.torrent_count} torrent{data.torrent_count > 1 ? "s" : ""}
        </span>
        <span className={overThreshold ? "text-amber-300" : "text-foreground"}>
          {usedPct.toFixed(1)}%
        </span>
      </div>
      <Progress value={usedPct} />
      <div className="flex justify-between text-[11px] text-muted-foreground">
        <span>
          target {data.target_pct}% ({formatSize(data.target_bytes)})
        </span>
        <span>
          threshold {data.threshold_pct}% ({formatSize(data.threshold_bytes)})
        </span>
      </div>
    </div>
  );
}

function UsersCard() {
  const users = useQuery({
    queryKey: ["admin", "users"],
    queryFn: admin.listUsers,
  });
  const [target, setTarget] = useState<UserView | null>(null);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Users</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        {users.isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : users.error ? (
          <p className="text-sm text-destructive">
            {users.error instanceof Error ? users.error.message : "failed"}
          </p>
        ) : users.data?.length ? (
          <div className="overflow-hidden rounded-md border border-border">
            <table className="w-full text-sm">
              <thead className="bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
                <tr>
                  <th className="px-4 py-2 text-left">Email</th>
                  <th className="px-4 py-2 text-left">Role</th>
                  <th className="px-4 py-2 text-left">Joined</th>
                  <th className="px-4 py-2"></th>
                </tr>
              </thead>
              <tbody>
                {users.data.map((u) => (
                  <tr key={u.id} className="border-t border-border">
                    <td className="break-all px-4 py-2">{u.email}</td>
                    <td className="px-4 py-2">
                      {u.is_admin ? (
                        <Badge
                          variant="outline"
                          className="inline-flex items-center gap-1 border-fuchsia-400/50 text-fuchsia-300"
                        >
                          <ShieldCheck className="size-3" />
                          admin
                        </Badge>
                      ) : (
                        <span className="text-xs text-muted-foreground">user</span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-xs text-muted-foreground">
                      {new Date(u.created_at).toLocaleDateString()}
                    </td>
                    <td className="px-4 py-2 text-right">
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setTarget(u)}
                        title="Reset password"
                      >
                        <KeyRound className="size-3.5" />
                        <span className="sr-only sm:not-sr-only sm:ml-1">Reset password</span>
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">No users yet.</p>
        )}
      </CardContent>
      <ResetPasswordDialog target={target} onOpenChange={(open) => !open && setTarget(null)} />
    </Card>
  );
}

function ResetPasswordDialog({
  target,
  onOpenChange,
}: {
  target: UserView | null;
  onOpenChange: (open: boolean) => void;
}) {
  const [pwd, setPwd] = useState("");
  const [confirm, setConfirm] = useState("");
  const [done, setDone] = useState(false);
  const reset = useMutation({
    mutationFn: ({ id, p }: { id: string; p: string }) => admin.resetPassword(id, p),
    onSuccess: () => {
      setDone(true);
      setPwd("");
      setConfirm("");
    },
  });

  // Reset state whenever the modal opens for a new user.
  const open = target != null;
  const onClose = (next: boolean) => {
    if (!next) {
      setPwd("");
      setConfirm("");
      setDone(false);
      reset.reset();
    }
    onOpenChange(next);
  };

  const localErr =
    pwd.length > 0 && pwd.length < 8
      ? "Password must be at least 8 characters."
      : confirm.length > 0 && pwd !== confirm
        ? "Passwords don't match."
        : null;
  const submitDisabled = !target || pwd.length < 8 || pwd !== confirm || reset.isPending;

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Reset password</DialogTitle>
          <DialogDescription>
            {target ? (
              <>
                For <span className="font-medium">{target.email}</span>.
              </>
            ) : null}{" "}
            All their existing sessions will be revoked.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-2">
            <Label htmlFor="adminPwd">New password</Label>
            <Input
              id="adminPwd"
              type="password"
              autoComplete="new-password"
              minLength={8}
              value={pwd}
              onChange={(e) => setPwd(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="adminPwd2">Confirm</Label>
            <Input
              id="adminPwd2"
              type="password"
              autoComplete="new-password"
              minLength={8}
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
            />
          </div>
          {localErr && <p className="text-sm text-destructive">{localErr}</p>}
          {reset.error && (
            <p className="text-sm text-destructive">
              {reset.error instanceof Error ? reset.error.message : "failed"}
            </p>
          )}
          {done && (
            <p className="text-sm text-emerald-300">
              Password updated. Hand the new password to {target?.email}.
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onClose(false)}>
            Close
          </Button>
          <Button
            disabled={submitDisabled}
            onClick={() => target && reset.mutate({ id: target.id, p: pwd })}
          >
            <KeyRound className="size-4" />
            {reset.isPending ? "Resetting…" : "Reset"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function GcReportView({ report }: { report: GcReport }) {
  const freed = report.used_bytes_before - report.used_bytes_after;
  return (
    <div className="rounded-md border border-border bg-muted/30 p-3 text-sm">
      <p className="text-muted-foreground">
        Freed {formatSize(freed)} ({report.evicted.length} torrent
        {report.evicted.length !== 1 ? "s" : ""} evicted)
      </p>
      {report.evicted.length > 0 && (
        <ul className="mt-2 grid gap-0.5 text-xs">
          {report.evicted.map((e) => (
            <li key={e.infohash} className="flex justify-between gap-2">
              <span className="truncate text-foreground">{e.name}</span>
              <span className="shrink-0 text-muted-foreground">−{formatSize(e.freed_bytes)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
