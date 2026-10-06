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

import { createRingBuffer, type RingBuffer } from './ring-buffer';

/** Vendored as a plain JS file in `public/`; Vite serves it as-is so
 *  the worklet's restricted execution context (no DOM, no Worker globals)
 *  doesn't fight TypeScript's lib choices. */
const PROCESSOR_URL = '/iris-audio-worklet.js';

export type AudioScheduler = {
	enqueue: (data: AudioData) => void;
	currentMediaTimeSeconds: () => number;
	outputLatencySeconds: () => number;
	setVolume: (vol01: number) => void;
	getVolume: () => number;
	setMuted: (muted: boolean) => void;
	getMuted: () => boolean;
	resetClock: () => void;
	/** False from a `resetClock` until the first buffer after it: the clock reads 0 meanwhile,
	 *  so callers show their seek target instead. */
	isAnchored: () => boolean;
	/** Resume the underlying AudioContext (autoplay policy leaves it
	 *  suspended until a user gesture). Safe to call repeatedly. */
	resume: () => void;
	/** Seconds of audio the context is holding between the graph and the
	 *  output device beyond what `outputLatency` admits — measured via
	 *  `getOutputTimestamp()` (Firefox under-reports `outputLatency` while
	 *  buffering hundreds of ms internally). Renderers chasing the media
	 *  clock must subtract this too, or the eye leads the ear by it. */
	hiddenOutputLagSeconds: () => number;
	/** Data consumed beyond elapsed output time — the browser's internal
	 *  output-pipeline pre-fill (0 where the clock already runs on wall
	 *  output time). Telemetry only. */
	pipelinePrefillSeconds: () => number;
	/** Suspend the AudioContext: consumption stops, so the media clock
	 *  freezes — everything chasing it (renderer, decode pacing) parks.
	 *  This IS pause for clock-driven engines. */
	suspend: () => void;
	dispose: () => Promise<void>;
};

export type AudioSchedulerOptions = {
	/** The source's rate: the context runs at it, so nothing is resampled (the device default,
	 *  often 44.1 kHz, would play a 48 kHz track at the wrong pitch and speed). */
	sampleRate?: number;
};

const DEFAULT_CAPACITY_SECONDS = 4;

function makeContext(sampleRate: number | undefined): AudioContext {
	if (sampleRate) {
		try {
			return new AudioContext({ sampleRate });
		} catch (e) {
			console.warn(`[iris-core] audio-scheduler: no AudioContext at ${sampleRate} Hz, using the device rate`, e);
		}
	}
	return new AudioContext();
}

export async function createAudioScheduler(opts: AudioSchedulerOptions = {}): Promise<AudioScheduler> {
	const ctx = makeContext(opts.sampleRate);
	const gain = ctx.createGain();
	gain.connect(ctx.destination);
	let currentVolume = 1;
	let muted = false;
	let rateWarned = false;

	const useWorklet = typeof SharedArrayBuffer !== 'undefined' && 'audioWorklet' in ctx;

	if (useWorklet) {
		try {
			const sched = await buildWorkletScheduler(ctx, gain);
			console.log(
				`[iris-core] audio-scheduler: worklet path, sr=${ctx.sampleRate} state=${ctx.state} ` +
					`baseLatency=${(ctx.baseLatency * 1000).toFixed(0)}ms outputLatency=${(readOutputLatency(ctx) * 1000).toFixed(0)}ms`
			);
			return sched;
		} catch (e) {
			console.warn('[iris-core] AudioWorklet path failed, falling back:', e);
		}
	}
	console.log(
		`[iris-core] audio-scheduler: legacy path, sr=${ctx.sampleRate} state=${ctx.state} ` +
			`baseLatency=${(ctx.baseLatency * 1000).toFixed(0)}ms outputLatency=${(readOutputLatency(ctx) * 1000).toFixed(0)}ms`
	);
	return buildLegacyScheduler(ctx, gain);

	/** One warning per scheduler, not one per AudioData (~47/s). */
	function warnRateOnce(dataRate: number, ctxRate: number): void {
		if (rateWarned) return;
		rateWarned = true;
		console.warn(`[iris-core] AudioData sampleRate ${dataRate} ≠ ctx ${ctxRate}; pitch will be wrong until resampler lands`);
	}

	function buildLegacyScheduler(ctx2: AudioContext, gain2: GainNode): AudioScheduler {
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
			} else if (playbackOrigin + mediaTimeSec < ctx2.currentTime + 0.02) {
				// The producer fell behind real time (an underrun played silence
				// while the wall clock ran on). Without re-anchoring, every such
				// gap shifts audio later than its timestamp FOREVER while video
				// keeps chasing the wall clock — A/V drift that only accumulates.
				// Shifting the origin makes the clock follow the audio CONTENT.
				playbackOrigin = ctx2.currentTime + 0.12 - mediaTimeSec;
				console.warn(`[iris-core] audio-scheduler: producer late — re-anchored clock (media=${mediaTimeSec.toFixed(2)}s)`);
			}
			const buffer = ctx2.createBuffer(numberOfChannels, numberOfFrames, sampleRate);
			for (let ch = 0; ch < numberOfChannels; ch += 1) {
				data.copyTo(buffer.getChannelData(ch), { planeIndex: ch, format: 'f32-planar' });
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
				playbackOrigin === null || playbackOrigin === undefined ? 0 : Math.max(0, ctx2.currentTime - playbackOrigin),
			outputLatencySeconds: () => readOutputLatency(ctx2),
			hiddenOutputLagSeconds: () => readHiddenOutputLag(ctx2),
			pipelinePrefillSeconds: () => 0,
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
			isAnchored: () => playbackOrigin !== null,
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
			resume: () => void ctx2.resume().catch(() => undefined),
			suspend: () => void ctx2.suspend().catch(() => undefined),
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
			}
		};
	}

	async function buildWorkletScheduler(ctx2: AudioContext, gain2: GainNode): Promise<AudioScheduler> {
		await ctx2.audioWorklet.addModule(PROCESSOR_URL);
		const capacityFrames = Math.ceil(ctx2.sampleRate * DEFAULT_CAPACITY_SECONDS);
		const ring: RingBuffer = createRingBuffer(capacityFrames);
		const node = new AudioWorkletNode(ctx2, 'iris-ring', {
			numberOfInputs: 0,
			numberOfOutputs: 1,
			outputChannelCount: [ring.channels]
		});
		node.port.postMessage({
			type: 'init',
			sab: ring.sab,
			channels: ring.channels,
			capacity: ring.capacity
		});
		node.connect(gain2);

		// Media clock derived from CONSUMPTION, not wall time: the worklet
		// plays whatever sits in the ring as soon as its callbacks run, so a
		// wall-clock projection (`ctx.currentTime + lead − mediaTime`) claims
		// times the speaker doesn't honour — anchoring while the context is
		// suspended (autoplay policy) had the audio physically LEADING the
		// clock (and thus the video) by the whole scheduling lead. Reading
		// the worklet's ring pointer gives the exact content the speaker is
		// consuming: suspension, late starts and underruns all freeze or
		// shift the clock to match reality automatically.
		let firstMediaTime: number | null = null;
		let consumedInterleaved = 0;
		let lastReadIdx = 0;
		/** `ctx.currentTime` when the worklet consumed its first sample.
		 *  Firefox pre-fills its internal output pipeline (AudioIPC) by
		 *  consuming hundreds of ms from the ring in a burst — consumed ≠
		 *  audible, and neither `outputLatency` nor `getOutputTimestamp`
		 *  admit that buffer. The audible position is therefore
		 *  `min(elapsed graph time since output started, consumed)`:
		 *  steady state follows real 1× output (pre-fill neutralised),
		 *  underruns clamp to actual data. */
		let ctxAtFirstConsume: number | null = null;
		/** Sum of content-time gaps between consecutively pushed chunks —
		 *  ring content is played back-to-back, so a producer-side gap
		 *  shifts every later chunk's audible time by the gap. */
		let gapSum = 0;
		let lastPushedEnd: number | null = null;
		let disposed = false;

		const consumedSeconds = (): number => {
			const r = ring.readIndex();
			let d = r - lastReadIdx;
			if (d < 0) d += ring.capacity;
			lastReadIdx = r;
			consumedInterleaved += d;
			if (consumedInterleaved > 0 && ctxAtFirstConsume === null) {
				ctxAtFirstConsume = ctx2.currentTime;
			}
			return consumedInterleaved / ring.channels / ctx2.sampleRate;
		};

		const enqueue = (data: AudioData): void => {
			if (disposed) {
				data.close();
				return;
			}
			const mediaTimeSec = data.timestamp / 1_000_000;
			if (firstMediaTime === null) {
				firstMediaTime = mediaTimeSec;
			} else if (lastPushedEnd !== null && mediaTimeSec - lastPushedEnd > 0.02) {
				gapSum += mediaTimeSec - lastPushedEnd;
				console.warn(
					`[iris-core] audio-scheduler: content gap ${((mediaTimeSec - lastPushedEnd) * 1000).toFixed(0)}ms ` +
						`at media=${mediaTimeSec.toFixed(2)}s — clock shifted`
				);
			}
			// The context runs at the source rate (see `makeContext`); a mismatch is a browser
			// that refused it. No resampler yet.
			if (data.sampleRate !== ctx2.sampleRate) warnRateOnce(data.sampleRate, ctx2.sampleRate);
			// A full ring keeps what it had: the frames that didn't fit count as a content gap
			// at the next push, which shifts the clock instead of letting it drift.
			const written = ring.push(data);
			if (written < data.numberOfFrames) {
				console.warn(
					`[iris-core] audio-scheduler: ring full, dropped ${data.numberOfFrames - written} frame(s) at media=${mediaTimeSec.toFixed(2)}s`
				);
			}
			lastPushedEnd = mediaTimeSec + written / data.sampleRate;
			data.close();
		};

		return {
			enqueue,
			currentMediaTimeSeconds: () => {
				if (firstMediaTime === null || firstMediaTime === undefined) return 0;
				const consumed = consumedSeconds();
				const elapsed =
					ctxAtFirstConsume === null || ctxAtFirstConsume === undefined ? 0 : Math.max(0, ctx2.currentTime - ctxAtFirstConsume);
				return firstMediaTime + gapSum + Math.min(consumed, elapsed);
			},
			outputLatencySeconds: () => readOutputLatency(ctx2),
			hiddenOutputLagSeconds: () => readHiddenOutputLag(ctx2),
			pipelinePrefillSeconds: () => {
				const consumed = consumedSeconds();
				const elapsed =
					ctxAtFirstConsume === null || ctxAtFirstConsume === undefined ? 0 : Math.max(0, ctx2.currentTime - ctxAtFirstConsume);
				return Math.max(0, consumed - elapsed);
			},
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
			isAnchored: () => firstMediaTime !== null,
			resetClock: () => {
				ring.reset();
				// Re-init the worklet so its read pointer aligns with our
				// post-reset write pointer (both are now zero).
				node.port.postMessage({
					type: 'init',
					sab: ring.sab,
					channels: ring.channels,
					capacity: ring.capacity
				});
				firstMediaTime = null;
				consumedInterleaved = 0;
				lastReadIdx = 0;
				gapSum = 0;
				lastPushedEnd = null;
				ctxAtFirstConsume = null;
			},
			resume: () => void ctx2.resume().catch(() => undefined),
			suspend: () => void ctx2.suspend().catch(() => undefined),
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
			}
		};
	}
}

function clamp01(v: number): number {
	return Math.max(0, Math.min(1, v));
}

/** Context-internal buffering `outputLatency` doesn't admit: the delta
 *  between the graph clock and the stream position actually leaving for
 *  the device (`getOutputTimestamp`). Clamped to [0, 2 s] — a suspended
 *  context or a browser without the API reports 0. */
function readHiddenOutputLag(ctx: AudioContext): number {
	const get = (ctx as AudioContext & { getOutputTimestamp?: () => { contextTime?: number } }).getOutputTimestamp?.bind(ctx);
	if (!get) return 0;
	const out = get();
	if (out?.contextTime === null || out?.contextTime === undefined || !Number.isFinite(out.contextTime)) return 0;
	const lag = ctx.currentTime - out.contextTime;
	if (!Number.isFinite(lag) || lag < 0) return 0;
	return Math.min(lag, 2);
}

function readOutputLatency(ctx: AudioContext): number {
	const ol = (ctx as AudioContext & { outputLatency?: number }).outputLatency;
	return typeof ol === 'number' ? ol : 0;
}
