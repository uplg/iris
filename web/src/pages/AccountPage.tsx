import { useMutation } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ApiError, auth as authApi } from "@/lib/api";
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

      <Card className="max-w-md">
        <CardHeader>
          <CardTitle>Change password</CardTitle>
          <CardDescription>
            Other sessions will be signed out automatically.
          </CardDescription>
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
                change.isPending ||
                !oldPwd ||
                !newPwd ||
                newPwd !== confirm ||
                newPwd.length < 8
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
