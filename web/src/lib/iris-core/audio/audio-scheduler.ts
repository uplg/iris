/**
 * Schedules decoded `AudioData` frames onto a Web Audio graph through
 * an `AudioWorklet` + `SharedArrayBuffer` ring buffer.
 *
 * The ring buffer is filled on the main thread by `enqueue` and
 * drained on the audio thread by the worklet processor. This gives
 * us sub-frame A/V sync (the audio thread runs at 128-sample blocks
 * ≈ 2.7ms) and predictable, low-GC throughput compared to the
 * one-`AudioBufferSourceNode`-per-chunk approach.
 *
 * The clock is anchored on the first `enqueue` after each `resetClock`
 * call (used after seeks). `currentMediaTimeSeconds()` returns the
 * absolute media timestamp of the audio about to leave the speaker.
 *
 * Falls back to the legacy per-chunk scheduler when SharedArrayBuffer
 * is unavailable (no cross-origin isolation, very old browsers). The
 * fallback's API surface is identical so callers don't need to know.
 */

import { createRingBuffer, type RingBuffer } from "./ring-buffer";

/** Vendored as a plain JS file in `public/`; Vite serves it as-is so
 *  the worklet's restricted execution context (no DOM, no Worker globals)
 *  doesn't fight TypeScript's lib choices. */
const PROCESSOR_URL = "/iris-audio-worklet.js";

export type AudioScheduler = {
  enqueue: (data: AudioData) => void;
  currentMediaTimeSeconds: () => number;
  outputLatencySeconds: () => number;
  setVolume: (vol01: number) => void;
  getVolume: () => number;
  setMuted: (muted: boolean) => void;
  getMuted: () => boolean;
  resetClock: () => void;
  dispose: () => Promise<void>;
};

export type AudioSchedulerOptions = {
  sampleRate?: number;
  channels?: number;
};

const DEFAULT_CHANNELS = 2;
const DEFAULT_CAPACITY_SECONDS = 4;

export async function createAudioScheduler(
  opts: AudioSchedulerOptions = {},
): Promise<AudioScheduler> {
  const ctx = new AudioContext(opts.sampleRate ? { sampleRate: opts.sampleRate } : undefined);
  const gain = ctx.createGain();
  gain.connect(ctx.destination);
  let currentVolume = 1;
  let muted = false;

  const useWorklet =
    typeof SharedArrayBuffer !== "undefined" && "audioWorklet" in ctx;

  if (useWorklet) {
    try {
      return await buildWorkletScheduler(ctx, gain, opts);
    } catch (e) {
      console.warn("[iris-core] AudioWorklet path failed, falling back:", e);
    }
  }
  return buildLegacyScheduler(ctx, gain);

  function buildLegacyScheduler(
    ctx2: AudioContext,
    gain2: GainNode,
  ): AudioScheduler {
    let playbackOrigin: number | null = null;
    const pendingSources: AudioBufferSourceNode[] = [];
    let disposed = false;

    const enqueue = (data: AudioData): void => {
      if (disposed) {
        data.close();
        return;
      }
      const { numberOfChannels, numberOfFrames, sampleRate } = data;
      const mediaTimeSec = data.timestamp / 1_000_000;
      if (playbackOrigin === null) {
        playbackOrigin = ctx2.currentTime + 0.12 - mediaTimeSec;
      }
      const buffer = ctx2.createBuffer(numberOfChannels, numberOfFrames, sampleRate);
      for (let ch = 0; ch < numberOfChannels; ch += 1) {
        data.copyTo(buffer.getChannelData(ch), { planeIndex: ch, format: "f32-planar" });
      }
      data.close();
      const node = ctx2.createBufferSource();
      node.buffer = buffer;
      node.connect(gain2);
      const target = Math.max(playbackOrigin + mediaTimeSec, ctx2.currentTime);
      node.start(target);
      node.onended = () => {
        const i = pendingSources.indexOf(node);
        if (i !== -1) pendingSources.splice(i, 1);
      };
      pendingSources.push(node);
    };
    return {
      enqueue,
      currentMediaTimeSeconds: () =>
        playbackOrigin == null ? 0 : Math.max(0, ctx2.currentTime - playbackOrigin),
      outputLatencySeconds: () => readOutputLatency(ctx2),
      setVolume: (v) => {
        currentVolume = clamp01(v);
        if (!muted) gain2.gain.setTargetAtTime(currentVolume, ctx2.currentTime, 0.01);
      },
      getVolume: () => currentVolume,
      setMuted: (m) => {
        muted = m;
        gain2.gain.setTargetAtTime(m ? 0 : currentVolume, ctx2.currentTime, 0.01);
      },
      getMuted: () => muted,
      resetClock: () => {
        for (const node of pendingSources) {
          try {
            node.stop();
          } catch {
            /* idempotent */
          }
        }
        pendingSources.length = 0;
        playbackOrigin = null;
      },
      dispose: async () => {
        if (disposed) return;
        disposed = true;
        for (const node of pendingSources) {
          try {
            node.stop();
          } catch {
            /* idempotent */
          }
        }
        pendingSources.length = 0;
        try {
          gain2.disconnect();
        } catch {
          /* idempotent */
        }
        try {
          await ctx2.close();
        } catch {
          /* idempotent */
        }
      },
    };
  }

  async function buildWorkletScheduler(
    ctx2: AudioContext,
    gain2: GainNode,
    schedOpts: AudioSchedulerOptions,
  ): Promise<AudioScheduler> {
    await ctx2.audioWorklet.addModule(PROCESSOR_URL);
    const channels = schedOpts.channels ?? DEFAULT_CHANNELS;
    const capacityFrames = Math.ceil(ctx2.sampleRate * DEFAULT_CAPACITY_SECONDS);
    let ring: RingBuffer = createRingBuffer(channels, capacityFrames);
    let node = new AudioWorkletNode(ctx2, "iris-ring", {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [channels],
    });
    node.port.postMessage({
      type: "init",
      sab: ring.sab,
      channels: ring.channels,
      capacity: ring.capacity,
    });
    node.connect(gain2);

    let playbackOrigin: number | null = null;
    let samplesWritten = 0;
    let disposed = false;

    const enqueue = (data: AudioData): void => {
      if (disposed) {
        data.close();
        return;
      }
      const mediaTimeSec = data.timestamp / 1_000_000;
      if (playbackOrigin === null) {
        playbackOrigin = ctx2.currentTime + 0.12 - mediaTimeSec;
      }
      // If the data's sampleRate differs from the AudioContext's, we
      // skip resampling for Phase 2 polish — most files are 48 kHz
      // and ctx defaults match. A proper resampler (OfflineAudioContext
      // or libsamplerate) is a follow-up.
      if (data.sampleRate !== ctx2.sampleRate) {
        console.warn(
          `[iris-core] AudioData sampleRate ${data.sampleRate} ≠ ctx ${ctx2.sampleRate}; pitch will be wrong until resampler lands`,
        );
      }
      ring.push(data);
      samplesWritten += data.numberOfFrames;
      data.close();
    };

    return {
      enqueue,
      currentMediaTimeSeconds: () => {
        if (playbackOrigin == null) return 0;
        return Math.max(0, ctx2.currentTime - playbackOrigin);
      },
      outputLatencySeconds: () => readOutputLatency(ctx2),
      setVolume: (v) => {
        currentVolume = clamp01(v);
        if (!muted) gain2.gain.setTargetAtTime(currentVolume, ctx2.currentTime, 0.01);
      },
      getVolume: () => currentVolume,
      setMuted: (m) => {
        muted = m;
        gain2.gain.setTargetAtTime(m ? 0 : currentVolume, ctx2.currentTime, 0.01);
      },
      getMuted: () => muted,
      resetClock: () => {
        ring.reset();
        // Re-init the worklet so its read pointer aligns with our
        // post-reset write pointer (both are now zero).
        node.port.postMessage({
          type: "init",
          sab: ring.sab,
          channels: ring.channels,
          capacity: ring.capacity,
        });
        playbackOrigin = null;
        samplesWritten = 0;
      },
      dispose: async () => {
        if (disposed) return;
        disposed = true;
        try {
          node.disconnect();
        } catch {
          /* idempotent */
        }
        try {
          gain2.disconnect();
        } catch {
          /* idempotent */
        }
        try {
          await ctx2.close();
        } catch {
          /* idempotent */
        }
        void samplesWritten; // referenced for future telemetry
      },
    };
  }
}

function clamp01(v: number): number {
  return Math.max(0, Math.min(1, v));
}

function readOutputLatency(ctx: AudioContext): number {
  const ol = (ctx as AudioContext & { outputLatency?: number }).outputLatency;
  return typeof ol === "number" ? ol : 0;
}
