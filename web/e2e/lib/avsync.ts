// A/V sync on the sync clip (a white flash and a beep onset at every whole second): when each
// flash reaches the screen and each beep onset leaves the audio output, on one clock
// (`performance.now()`), whatever the tier.
//
// Picture: an element tier's frames through `requestVideoFrameCallback` (`expectedDisplayTime`,
// the compositor's own prediction); a canvas tier's (C/D, on the Canvas2D renderer: WebGPU is
// hidden for these specs) at the `drawImage` of the frame, plus one 60 Hz frame for its display.
// Sound: an AudioWorklet that timestamps the onset to the sample, mapped to `performance.now()`
// through `getOutputTimestamp()` (the moment that context time reaches the output). An element's
// audio is tapped through `captureStream()`; a Web Audio tier's off the bench's own tap on its
// destination, in its own context.
//
// What no page can see is the hardware past the output (the speaker, a Bluetooth link): the
// numbers are the browser's pipeline, which is where the tiers differ.

export interface AvSync {
	flashes: number[];
	onsets: number[];
	mode: string;
}

/** Before any page script: the canvas-tier picture hook, and no WebGPU (the Canvas2D renderer
 * draws where the hook can see it). */
export function installAvSync() {
	const w = window as unknown as { __avs: AvSync & { canvasArmed: boolean; lastLum: number } };
	w.__avs = { flashes: [], onsets: [], mode: 'idle', canvasArmed: false, lastLum: 0 };
	Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined, configurable: true });
	const draw = CanvasRenderingContext2D.prototype.drawImage as (this: CanvasRenderingContext2D, ...a: unknown[]) => void;
	CanvasRenderingContext2D.prototype.drawImage = function (this: CanvasRenderingContext2D, ...a: unknown[]) {
		draw.apply(this, a);
		const s = w.__avs;
		if (!s.canvasArmed || typeof VideoFrame === 'undefined' || !(a[0] instanceof VideoFrame)) return;
		const px = this.getImageData(this.canvas.width >> 1, this.canvas.height >> 1, 1, 1).data;
		const lum = (px[0]! + px[1]! + px[2]!) / 3;
		if (lum > 128 && s.lastLum <= 128) {
			const frameStart = (document.timeline.currentTime as number | null) ?? performance.now();
			s.flashes.push(frameStart + 1000 / 60);
		}
		s.lastLum = lum;
	} as typeof CanvasRenderingContext2D.prototype.drawImage;
}

/** Starts both detectors on whatever the engine shows and plays. */
export async function startAvSync(): Promise<string> {
	const w = window as unknown as {
		__avs: AvSync & { canvasArmed: boolean; lastLum: number };
		__benchTaps?: { left: AnalyserNode; ctx: BaseAudioContext }[];
	};
	const s = w.__avs;
	const worklet = `
		class Onset extends AudioWorkletProcessor {
			constructor() { super(); this.armed = true; this.quiet = 0; }
			process(inputs) {
				const ch = inputs[0] && inputs[0][0];
				if (!ch) return true;
				for (let i = 0; i < ch.length; i++) {
					const a = Math.abs(ch[i]);
					if (this.armed) {
						if (a > 0.15) { this.port.postMessage(currentFrame + i); this.armed = false; this.quiet = 0; }
					} else if (a < 0.02) {
						if (++this.quiet > sampleRate * 0.3) this.armed = true;
					} else this.quiet = 0;
				}
				return true;
			}
		}
		registerProcessor('iris-onset', Onset);`;
	const wire = async (ctx: AudioContext, source: AudioNode) => {
		await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([worklet], { type: 'text/javascript' })));
		const node = new AudioWorkletNode(ctx, 'iris-onset');
		const sink = ctx.createGain();
		sink.gain.value = 0;
		source.connect(node);
		node.connect(sink);
		sink.connect(ctx.destination);
		node.port.onmessage = (e) => {
			const ts = ctx.getOutputTimestamp();
			const at = (e.data as number) / ctx.sampleRate;
			s.onsets.push((ts.performanceTime ?? performance.now()) + (at - (ts.contextTime ?? ctx.currentTime)) * 1000);
		};
		if (ctx.state !== 'running') await ctx.resume();
	};

	const video = document.querySelector<HTMLVideoElement>('.video-host video');
	if (video && (video.src || video.srcObject || video.currentSrc)) {
		let last = 0;
		const probe = document.createElement('canvas');
		probe.width = probe.height = 4;
		const pctx = probe.getContext('2d', { willReadFrequently: true })!;
		const onFrame = (_now: number, md: VideoFrameCallbackMetadata) => {
			pctx.drawImage(video, 0, 0, 4, 4);
			const px = pctx.getImageData(1, 1, 1, 1).data;
			const lum = (px[0]! + px[1]! + px[2]!) / 3;
			if (lum > 128 && last <= 128) s.flashes.push(md.expectedDisplayTime);
			last = lum;
			video.requestVideoFrameCallback(onFrame);
		};
		video.requestVideoFrameCallback(onFrame);
		const el = video as HTMLVideoElement & { mozCaptureStream?: () => MediaStream; captureStream?: () => MediaStream };
		const ctx = new AudioContext();
		if (el.mozCaptureStream || el.captureStream) {
			const capture = el.mozCaptureStream ? el.mozCaptureStream() : el.captureStream!();
			await wire(ctx, ctx.createMediaStreamSource(capture));
			s.mode = 'element';
		} else {
			// WebKit has no captureStream on media elements: route the element through Web Audio
			// (it still plays, from this context), and time it there
			const src = ctx.createMediaElementSource(el);
			src.connect(ctx.destination);
			await wire(ctx, src);
			s.mode = 'element via Web Audio';
		}
	} else {
		s.canvasArmed = true;
		const tap = w.__benchTaps?.[0];
		if (!tap) throw new Error('no Web Audio tap: the engine has not wired its output');
		await wire(tap.ctx as AudioContext, tap.left);
		s.mode = 'canvas';
	}
	return s.mode;
}

export function readAvSync(): AvSync {
	const s = (window as unknown as { __avs: AvSync }).__avs;
	return { flashes: [...s.flashes], onsets: [...s.onsets], mode: s.mode };
}

export interface SyncStats {
	pairs: number;
	/** audio minus picture, ms: positive = the sound comes late */
	median: number;
	p90abs: number;
	maxAbs: number;
	/** least-squares slope of the offset over the session, ms per minute */
	driftPerMin: number;
	offsets: number[];
}

/** Pairs every beep onset with the flash nearest to it (within 400 ms). */
export function syncStats(a: AvSync): SyncStats {
	const offsets: number[] = [];
	const at: number[] = [];
	for (const o of a.onsets) {
		let best: number | null = null;
		for (const f of a.flashes) if (best === null || Math.abs(o - f) < Math.abs(o - best)) best = f;
		if (best !== null && Math.abs(o - best) < 400) {
			offsets.push(o - best);
			at.push(best);
		}
	}
	const sorted = offsets.toSorted((x, y) => x - y);
	const abs = offsets.map(Math.abs).toSorted((x, y) => x - y);
	const n = offsets.length;
	let slope = 0;
	if (n > 2) {
		const mx = at.reduce((p, v) => p + v, 0) / n;
		const my = offsets.reduce((p, v) => p + v, 0) / n;
		let num = 0;
		let den = 0;
		for (let i = 0; i < n; i++) {
			num += (at[i]! - mx) * (offsets[i]! - my);
			den += (at[i]! - mx) ** 2;
		}
		slope = den > 0 ? (num / den) * 60_000 : 0;
	}
	return {
		pairs: n,
		median: n ? sorted[n >> 1]! : NaN,
		p90abs: n ? abs[Math.min(n - 1, Math.floor(n * 0.9))]! : NaN,
		maxAbs: n ? abs[n - 1]! : NaN,
		driftPerMin: slope,
		offsets
	};
}
