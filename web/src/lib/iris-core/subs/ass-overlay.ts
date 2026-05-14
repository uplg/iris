/**
 * ASS / SSA subtitle overlay via `libass-wasm` (a.k.a. SubtitlesOctopus).
 *
 * The library expects to be loaded as a `<script>` tag — the upstream
 * package ships a UMD bundle, not an ES module, and the worker + WASM
 * sit alongside it. We vendor the dist into `web/public/libass/` and
 * inject the script lazily on first use so the 1.6 MiB initial WASM
 * binary doesn't bloat the bundle.
 *
 * The renderer keeps an internal `<canvas>` aligned with a parent
 * `<video>` (or any element whose `currentTime` is read) and re-paints
 * on every `timeupdate`. We expose a small `mountAssOverlay` helper
 * that wires it to a generic timestamp getter so non-`<video>`
 * surfaces (Tier C's canvas) work too.
 */

const LIBASS_BASE = "/libass";
const SCRIPT_URL = `${LIBASS_BASE}/subtitles-octopus.js`;
const WORKER_URL = `${LIBASS_BASE}/subtitles-octopus-worker.js`;
const LEGACY_WORKER_URL = `${LIBASS_BASE}/subtitles-octopus-worker-legacy.js`;
const FALLBACK_FONT_URL = `${LIBASS_BASE}/default.woff2`;

declare global {
  // SubtitlesOctopus is attached to window when the script loads.
  // We type only the surface we actually call.
  interface Window {
    SubtitlesOctopus?: SubtitlesOctopusConstructor;
  }
}

type SubtitlesOctopusOptions = {
  video?: HTMLVideoElement;
  canvas?: HTMLCanvasElement;
  subUrl?: string;
  subContent?: string;
  workerUrl: string;
  legacyWorkerUrl?: string;
  fallbackFont?: string;
  fonts?: string[];
  availableFonts?: Record<string, string>;
  timeOffset?: number;
  debug?: boolean;
  onReady?: () => void;
  onError?: (err: unknown) => void;
};

type SubtitlesOctopusInstance = {
  setCurrentTime(time: number): void;
  setTrackByUrl(url: string): void;
  resize(width: number, height: number, offsetX?: number, offsetY?: number): void;
  dispose(): void;
};

type SubtitlesOctopusConstructor = new (
  options: SubtitlesOctopusOptions,
) => SubtitlesOctopusInstance;

let scriptLoadPromise: Promise<SubtitlesOctopusConstructor> | null = null;

async function loadSubtitlesOctopus(): Promise<SubtitlesOctopusConstructor> {
  if (window.SubtitlesOctopus) return window.SubtitlesOctopus;
  if (scriptLoadPromise) return scriptLoadPromise;
  scriptLoadPromise = new Promise<SubtitlesOctopusConstructor>((resolve, reject) => {
    const tag = document.createElement("script");
    tag.src = SCRIPT_URL;
    tag.async = true;
    tag.onload = () => {
      const ctor = window.SubtitlesOctopus;
      if (ctor) resolve(ctor);
      else reject(new Error("libass-wasm loaded but SubtitlesOctopus is undefined"));
    };
    tag.onerror = () => reject(new Error(`failed to load ${SCRIPT_URL}`));
    document.head.appendChild(tag);
  });
  return scriptLoadPromise;
}

export type AssOverlayOptions = {
  /** The element the overlay canvas should be positioned over. */
  host: HTMLElement;
  /** Server URL that returns the raw ASS bytes. */
  subUrl: string;
  /** Function returning the current media time in seconds. Phase 2d
   *  alpha just polls this via rAF; native `<video>` time sync (which
   *  libass-wasm does internally) lands when we accept a `<video>`. */
  getCurrentTime: () => number;
  /** Native `<video>` element to bind to. If provided, libass-wasm
   *  drives its own timeupdate loop and ignores `getCurrentTime`. */
  video?: HTMLVideoElement;
};

export type AssOverlayHandle = {
  dispose: () => void;
};

/**
 * Toggle libass internal logging via a localStorage flag. Set
 * `localStorage.setItem("iris.debug.libass", "1")` in DevTools and
 * reload to see the per-frame trace coming out of the worker — the
 * fontconfig / font-substitution / error paths all log there. Off
 * by default in prod to keep the console clean.
 */
function libassDebugEnabled(): boolean {
  try {
    return localStorage.getItem("iris.debug.libass") === "1";
  } catch {
    return false;
  }
}

export async function mountAssOverlay(opts: AssOverlayOptions): Promise<AssOverlayHandle> {
  const Ctor = await loadSubtitlesOctopus();

  // Create an absolutely-positioned canvas inside the host. CSS keeps
  // it identical in size to the host's content box; libass-wasm resizes
  // the bitmap to match on every paint.
  const canvas = document.createElement("canvas");
  canvas.className = "pointer-events-none absolute inset-0 h-full w-full";
  opts.host.appendChild(canvas);

  const debug = libassDebugEnabled();
  if (debug) {
    console.info("[iris-core:libass] mounting overlay", {
      subUrl: opts.subUrl,
      hasVideo: !!opts.video,
      workerUrl: WORKER_URL,
    });
  }
  // Live worker reference. Reassigned on auto-remount (see below).
  // `let` because the recovery path swaps the binding rather than
  // mutating the existing object (whose methods may live on the
  // prototype — `Object.assign` wouldn't copy them).
  let instance: SubtitlesOctopusInstance;

  const constructorOptions = (
    onError: (err: unknown) => void,
  ): SubtitlesOctopusOptions => ({
    video: opts.video,
    canvas,
    subUrl: opts.subUrl,
    workerUrl: WORKER_URL,
    legacyWorkerUrl: LEGACY_WORKER_URL,
    fallbackFont: FALLBACK_FONT_URL,
    debug,
    onReady: debug
      ? () => console.info("[iris-core:libass] worker ready")
      : undefined,
    onError,
  });

  // Track worker errors so we can auto-remount on transient failures.
  // libass-wasm sometimes wedges itself on the first cue if the worker
  // raced its own initialisation (seen on Windows browsers with
  // intermittent SharedArrayBuffer / cross-origin-isolation quirks).
  // A one-shot remount-after-error usually unsticks it; if the
  // problem is structural the second mount fails too and we give up
  // without flooding the console.
  let remountAttempts = 0;
  const MAX_REMOUNTS = 1;
  const handleWorkerError = (err: unknown) => {
    console.error("[iris-core:libass] worker error", err);
    if (remountAttempts >= MAX_REMOUNTS) return;
    remountAttempts += 1;
    if (debug) {
      console.info(
        `[iris-core:libass] remount attempt ${remountAttempts}/${MAX_REMOUNTS}`,
      );
    }
    try {
      instance.dispose();
    } catch {
      /* recovery path — original instance is already broken */
    }
    // Defer one frame so the worker thread fully tears down before
    // attaching a new one (constructor refetches the worker script).
    requestAnimationFrame(() => {
      try {
        instance = new Ctor(constructorOptions(handleWorkerError));
        syncResize();
      } catch (e) {
        console.error("[iris-core:libass] remount failed", e);
      }
    });
  };

  // Any error path the worker hits (font load failure, malformed
  // ASS, parse exception, missing SharedArrayBuffer, …) used to
  // silently disable the overlay because we never wired this
  // callback. Now logged + one transparent remount.
  instance = new Ctor(constructorOptions(handleWorkerError));

  // When there's no <video>, drive the time pump ourselves via rAF.
  let rafId: number | null = null;
  if (!opts.video) {
    const tick = () => {
      try {
        instance.setCurrentTime(opts.getCurrentTime());
      } catch {
        /* libass may throw during teardown — swallow */
      }
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
  }

  // Forced initial resize: ResizeObserver fires on layout *change*, not
  // initial mount, so libass would otherwise stay at its default
  // bitmap size until the host element resized (window resize,
  // fullscreen toggle, …). On Windows with display scaling that's a
  // visible window where subtitles render to a tiny canvas, get
  // scaled up, and look "missing" / blurry. Read the host's
  // bounding box immediately and forward — if it's still 0×0 the
  // observer takes over.
  const syncResize = () => {
    const rect = opts.host.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      try {
        instance.resize(rect.width, rect.height);
      } catch {
        /* ignore mid-disposal resize */
      }
    }
  };
  syncResize();

  // Resize observer: libass needs the canvas to match the host's pixel
  // size to avoid blurry / aliased text. Use a ResizeObserver and forward.
  const observer = new ResizeObserver(syncResize);
  observer.observe(opts.host);

  return {
    dispose: () => {
      observer.disconnect();
      if (rafId !== null) cancelAnimationFrame(rafId);
      try {
        instance.dispose();
      } catch {
        /* idempotent */
      }
      if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
    },
  };
}
