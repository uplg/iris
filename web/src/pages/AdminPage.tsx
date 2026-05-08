import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Plus, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { admin, type CreatedInvitation, type GcReport, type Invitation } from "@/lib/api";
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
    </div>
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
