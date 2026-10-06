/**
 * Main-thread producer for the AudioWorklet ring buffer. Allocates a
 * `SharedArrayBuffer`, exposes a `push(AudioData)` that mixes the source
 * channels down to the ring's stereo and advances the write pointer atomically.
 *
 * Layout:
 *   header: [read_idx, write_idx, channels, capacity_interleaved]  (4× i32)
 *   payload: Float32 interleaved samples, ring-indexed
 *
 * Capacity is in *interleaved samples* (not frames). Default is 4
 * seconds at 48 kHz stereo = ~1.5 MB.
 */

import { mixToStereo, stereoWeights, type StereoWeights } from './downmix';

const HEADER_SIZE = 4;
const READ_IDX = 0;
const WRITE_IDX = 1;
const CHANNELS_IDX = 2;
const CAPACITY_IDX = 3;

/** What `push` reads of an `AudioData`. */
export type PcmSource = {
	numberOfFrames: number;
	numberOfChannels: number;
	copyTo: (dest: Float32Array, options: { planeIndex: number; format: 'f32-planar' }) => void;
};

export type RingBuffer = {
	sab: SharedArrayBuffer;
	/** Always 2: the ring holds stereo, whatever the source layout. */
	channels: number;
	/** Interleaved-sample capacity. */
	capacity: number;
	/** Writes what fits; returns the frames written. A producer that keeps pushing into a
	 *  full ring would overwrite the samples the worklet is about to play. */
	push: (data: PcmSource) => number;
	reset: () => void;
	/** Free space in interleaved samples. */
	free: () => number;
	/** Frames written and not yet played. */
	bufferedFrames: () => number;
	/** The worklet's read pointer (interleaved samples, ring-indexed).
	 *  The scheduler derives its media clock from consumption deltas. */
	readIndex: () => number;
};

const CHANNELS = 2;

export function createRingBuffer(capacityFrames: number): RingBuffer {
	const capacity = capacityFrames * CHANNELS;
	const sab = new SharedArrayBuffer(HEADER_SIZE * 4 + capacity * 4);
	const header = new Int32Array(sab, 0, HEADER_SIZE);
	const samples = new Float32Array(sab, HEADER_SIZE * 4);
	Atomics.store(header, CHANNELS_IDX, CHANNELS);
	Atomics.store(header, CAPACITY_IDX, capacity);

	// Reusable scratch for AudioData copyTo — one f32 array per source channel.
	let scratch: Float32Array[] = [];
	let weights: StereoWeights = stereoWeights(CHANNELS);
	let weightsFor = CHANNELS;

	const used = (): number => {
		const r = Atomics.load(header, READ_IDX);
		const w = Atomics.load(header, WRITE_IDX);
		const u = w - r;
		return u < 0 ? u + capacity : u;
	};
	// leave one frame to tell full from empty
	const free = (): number => capacity - used() - CHANNELS;

	const push = (data: PcmSource): number => {
		const frames = Math.min(data.numberOfFrames, Math.max(0, Math.floor(free() / CHANNELS)));
		if (frames <= 0) return 0;
		const n = data.numberOfChannels;
		if (scratch.length !== n) scratch = Array.from({ length: n }, () => new Float32Array(0));
		for (let c = 0; c < n; c += 1) {
			// copyTo needs room for the whole plane, not only what we keep
			if (scratch[c]!.length < data.numberOfFrames) scratch[c] = new Float32Array(data.numberOfFrames);
			data.copyTo(scratch[c]!, { planeIndex: c, format: 'f32-planar' });
		}
		if (weightsFor !== n) {
			weights = stereoWeights(n);
			weightsFor = n;
		}
		const write = Atomics.load(header, WRITE_IDX);
		mixToStereo(scratch, frames, weights, samples, write);
		Atomics.store(header, WRITE_IDX, (write + frames * CHANNELS) % capacity);
		return frames;
	};

	const reset = (): void => {
		Atomics.store(header, READ_IDX, 0);
		Atomics.store(header, WRITE_IDX, 0);
	};

	return {
		sab,
		channels: CHANNELS,
		capacity,
		push,
		reset,
		free,
		bufferedFrames: () => used() / CHANNELS,
		readIndex: () => Atomics.load(header, READ_IDX)
	};
}
