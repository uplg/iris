import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";
import { ArrowUpRight } from "lucide-react";
import { Brand } from "@/components/Brand";
import { ThemeToggle } from "@/components/ThemeToggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/lib/auth";
import { ApiError, IRIS_WEB_VERSION } from "@/lib/api";

/** Clean accent-gradient panel for the auth pages (login + register). */
export function AuthVisual() {
  return (
    <aside className="relative hidden flex-col justify-between overflow-hidden bg-elev p-12 lg:flex">
      <div
        aria-hidden
        className="absolute inset-0"
        style={{
          background:
            "radial-gradient(900px 600px at 25% 100%, var(--brand-3) 0%, transparent 55%), radial-gradient(900px 500px at 100% 0%, var(--brand-2) 0%, transparent 60%), radial-gradient(700px 500px at 50% 55%, var(--brand) 0%, transparent 60%), linear-gradient(135deg, oklch(0.18 0.02 280) 0%, oklch(0.13 0.015 260) 100%)",
          opacity: 0.85,
          filter: "saturate(1.1)",
        }}
      />
      <div
        aria-hidden
        className="absolute inset-0 mix-blend-overlay"
        style={{
          background:
            "repeating-linear-gradient(135deg, oklch(0 0 0 / 0.05) 0 2px, transparent 2px 14px)",
        }}
      />

      <Brand size="md" asLink={false} />

      <h2
        className="display relative text-white"
        style={{ fontSize: "clamp(36px, 4vw, 52px)", textShadow: "0 2px 24px oklch(0 0 0 / 0.3)" }}
      >
        Your library,
        <br />
        untethered.
      </h2>

      <span className="relative text-xs text-white/55">v{IRIS_WEB_VERSION} · self-hosted</span>
    </aside>
  );
}

export function LoginPage() {
  const auth = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  if (auth.status === "authenticated") {
    const from = (location.state as { from?: { pathname: string } } | null)?.from?.pathname ?? "/";
    return <Navigate to={from} replace />;
  }

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setPending(true);
    setError(null);
    try {
      await auth.login(email, password);
      navigate("/", { replace: true });
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "login failed");
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
            <span className="eyebrow">Welcome back</span>
            <h1 className="display text-[40px]">Sign in</h1>
            <p className="text-[14.5px] text-muted-foreground">Use your Iris credentials.</p>
          </div>

          <form onSubmit={onSubmit} className="grid gap-4">
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
              <div className="flex items-baseline justify-between">
                <Label htmlFor="password" className="text-[12.5px] text-muted-foreground">
                  Password
                </Label>
              </div>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                required
                className="h-13 text-base"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <Button type="submit" size="lg" disabled={pending} className="h-11">
              {pending ? "Signing in…" : "Sign in"}
              {!pending && <ArrowUpRight className="size-4" />}
            </Button>
          </form>

          <div className="flex items-center gap-3">
            <div className="h-px flex-1 bg-border" />
            <span className="text-xs text-fg-dim">or</span>
            <div className="h-px flex-1 bg-border" />
          </div>

          <p className="text-center text-[13.5px] text-muted-foreground">
            Have an invitation?{" "}
            <Link
              to="/register"
              className="text-foreground underline underline-offset-[3px] hover:text-primary"
            >
              Create your account
            </Link>
          </p>
        </div>
      </section>
    </div>
  );
}
