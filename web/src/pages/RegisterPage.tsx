import { useEffect, useState, type FormEvent } from "react";
import { getRouteApi, Link, Navigate, useNavigate } from "@tanstack/react-router";
import { ArrowUpRight } from "lucide-react";
import { Brand } from "@/components/Brand";
import { ThemeToggle } from "@/components/ThemeToggle";
import { AuthVisual } from "@/pages/LoginPage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/lib/auth";
import { ApiError } from "@/lib/api";

const registerRoute = getRouteApi("/register");

export function RegisterPage() {
  const auth = useAuth();
  const navigate = useNavigate();
  const { token: tokenParam } = registerRoute.useSearch();
  const [token, setToken] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (tokenParam) setToken(tokenParam);
  }, [tokenParam]);

  if (auth.status === "authenticated") {
    return <Navigate to="/" replace />;
  }

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setPending(true);
    setError(null);
    try {
      await auth.register(token, email, password);
      navigate({ to: "/", replace: true });
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "registration failed");
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="grid min-h-svh lg:grid-cols-2">
      <AuthVisual />

      <section className="relative flex items-center justify-center p-6 sm:p-12">
        <div className="absolute right-4 top-4">
          <ThemeToggle />
        </div>
        <div className="grid w-full max-w-95 gap-6">
          <div className="lg:hidden">
            <Brand size="md" asLink={false} />
          </div>

          <div className="grid gap-1.5">
            <span className="eyebrow">Invitation only</span>
            <h1 className="display text-[40px]">Create account</h1>
            <p className="text-[14.5px] text-muted-foreground">
              An admin must give you an invite token.
            </p>
          </div>

          <form onSubmit={onSubmit} className="grid gap-4">
            <div className="grid gap-1.5">
              <Label htmlFor="token" className="text-[12.5px] text-muted-foreground">
                Invitation token
              </Label>
              <Input
                id="token"
                type="text"
                required
                className="h-13 font-mono text-base"
                value={token}
                onChange={(e) => setToken(e.target.value)}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="email" className="text-[12.5px] text-muted-foreground">
                Email
              </Label>
              <Input
                id="email"
                type="email"
                autoComplete="email"
                required
                className="h-13 text-base"
                placeholder="you@iris.local"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="password" className="text-[12.5px] text-muted-foreground">
                Password (min 8 chars)
              </Label>
              <Input
                id="password"
                type="password"
                autoComplete="new-password"
                minLength={8}
                required
                className="h-13 text-base"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <Button type="submit" size="lg" disabled={pending} className="h-11">
              {pending ? "Creating…" : "Create account"}
              {!pending && <ArrowUpRight className="size-4" />}
            </Button>
          </form>

          <div className="flex items-center gap-3">
            <div className="h-px flex-1 bg-border" />
            <span className="text-xs text-fg-dim">or</span>
            <div className="h-px flex-1 bg-border" />
          </div>

          <p className="text-center text-[13.5px] text-muted-foreground">
            Already have an account?{" "}
            <Link
              to="/login"
              className="text-foreground underline underline-offset-[3px] hover:text-primary"
            >
              Sign in
            </Link>
          </p>
        </div>
      </section>
    </div>
  );
}
