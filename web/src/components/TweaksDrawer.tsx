import { useEffect } from "react";
import { Moon, Sun, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { ACCENTS, ACCENT_SWATCH, type Accent, type Density, useTheme } from "@/lib/theme";

/**
 * The "Display" panel opened from the header cog. Lets a user flip theme
 * (dark/light), accent colour, and layout density — all persisted prefs that
 * apply across every page. Anchored top-right under the header, like the mock.
 */
export function TweaksDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { resolved, setTheme, accent, setAccent, density, setDensity } = useTheme();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      role="presentation"
      onClick={onClose}
      className="fixed inset-0 z-50 grid items-start justify-items-end bg-black/40 p-4 backdrop-blur-sm"
      style={{ paddingTop: "calc(var(--header-h) + 8px)" }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Display settings"
        onClick={(e) => e.stopPropagation()}
        className="grid w-[min(360px,calc(100vw-2rem))] gap-4 rounded-xl border border-border bg-elev p-[18px] shadow-2xl"
      >
        <div className="flex items-center justify-between">
          <span className="heading-3">Display</span>
          <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="Close">
            <X className="size-3.5" />
          </Button>
        </div>

        <Field label="Theme">
          <div className="grid grid-cols-2 gap-1.5">
            <Button
              variant={resolved === "dark" ? "default" : "secondary"}
              onClick={() => setTheme("dark")}
            >
              <Moon className="size-3.5" /> Dark
            </Button>
            <Button
              variant={resolved === "light" ? "default" : "secondary"}
              onClick={() => setTheme("light")}
            >
              <Sun className="size-3.5" /> Light
            </Button>
          </div>
        </Field>

        <Field label="Accent">
          <AccentSwatches value={accent} onChange={setAccent} />
        </Field>

        <Field label="Density">
          <div className="grid grid-cols-2 gap-1.5">
            <DensityButton value="comfortable" current={density} onChange={setDensity}>
              Comfortable
            </DensityButton>
            <DensityButton value="compact" current={density} onChange={setDensity}>
              Compact
            </DensityButton>
          </div>
        </Field>

        <p className="text-[11.5px] leading-relaxed text-fg-dim">
          Persisted preferences — applied across every page of Iris.
        </p>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-2">
      <span className="eyebrow">{label}</span>
      {children}
    </div>
  );
}

export function AccentSwatches({
  value,
  onChange,
}: {
  value: Accent;
  onChange: (a: Accent) => void;
}) {
  return (
    <div className="flex gap-2">
      {ACCENTS.map((a) => (
        <button
          key={a}
          type="button"
          onClick={() => onChange(a)}
          aria-label={a}
          aria-pressed={value === a}
          className={cn(
            "size-8 rounded-full border-2 transition focus-ring",
            value === a ? "border-foreground ring-2 ring-elev" : "border-border",
          )}
          style={{ background: ACCENT_SWATCH[a] }}
        />
      ))}
    </div>
  );
}

function DensityButton({
  value,
  current,
  onChange,
  children,
}: {
  value: Density;
  current: Density;
  onChange: (d: Density) => void;
  children: React.ReactNode;
}) {
  return (
    <Button variant={current === value ? "default" : "secondary"} onClick={() => onChange(value)}>
      {children}
    </Button>
  );
}
