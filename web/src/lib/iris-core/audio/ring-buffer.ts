/**
 * Main-thread producer for the AudioWorklet ring buffer. Allocates a
 * `SharedArrayBuffer`, exposes a `push(AudioData)` that interleaves
 * the channels and advances the write pointer atomically.
 *
 * Layout:
 *   header: [read_idx, write_idx, channels, capacity_interleaved]  (4× i32)
 *   payload: Float32 interleaved samples, ring-indexed
 *
 * Capacity is in *interleaved samples* (not frames). Default is 4
 * seconds at 48 kHz stereo = ~1.5 MB.
 */

const HEADER_SIZE = 4;
const READ_IDX = 0;
const WRITE_IDX = 1;
const CHANNELS_IDX = 2;
const CAPACITY_IDX = 3;

export type RingBuffer = {
  sab: SharedArrayBuffer;
  channels: number;
  /** Interleaved-sample capacity. */
  capacity: number;
  push: (data: AudioData) => void;
  reset: () => void;
  /** Free space in interleaved samples. */
  free: () => number;
  /** Whether the buffer is more than 75% full — back-pressure signal
   *  for the decode pipeline. */
  near_full: () => boolean;
};

export function createRingBuffer(
  channels: number,
  capacityFrames: number,
): RingBuffer {
  const capacity = capacityFrames * channels;
  const sab = new SharedArrayBuffer(HEADER_SIZE * 4 + capacity * 4);
  const header = new Int32Array(sab, 0, HEADER_SIZE);
  const samples = new Float32Array(sab, HEADER_SIZE * 4);
  Atomics.store(header, CHANNELS_IDX, channels);
  Atomics.store(header, CAPACITY_IDX, capacity);

  // Reusable scratch for AudioData copyTo — one f32 array per channel.
  const scratch: Float32Array[] = Array.from({ length: channels }, () => new Float32Array(0));

  const ensureScratch = (frames: number) => {
    for (let c = 0; c < channels; c += 1) {
      if (scratch[c]!.length < frames) scratch[c] = new Float32Array(frames);
    }
  };

  const push = (data: AudioData): void => {
    const frames = data.numberOfFrames;
    const dataChannels = Math.min(data.numberOfChannels, channels);
    ensureScratch(frames);
    for (let c = 0; c < dataChannels; c += 1) {
      data.copyTo(scratch[c]!, { planeIndex: c, format: "f32-planar" });
    }
    // Interleave straight into the ring at the write pointer.
    let write = Atomics.load(header, WRITE_IDX);
    for (let i = 0; i < frames; i += 1) {
      for (let c = 0; c < channels; c += 1) {
        samples[(write + c) % capacity] = c < dataChannels ? scratch[c]![i]! : 0;
      }
      write = (write + channels) % capacity;
    }
    Atomics.store(header, WRITE_IDX, write);
  };

  const reset = (): void => {
    Atomics.store(header, READ_IDX, 0);
    Atomics.store(header, WRITE_IDX, 0);
  };

  const free = (): number => {
    const r = Atomics.load(header, READ_IDX);
    const w = Atomics.load(header, WRITE_IDX);
    let used = w - r;
    if (used < 0) used += capacity;
    return capacity - used - channels; // leave one frame to disambiguate full/empty
  };

  return {
    sab,
    channels,
    capacity,
    push,
    reset,
    free,
    near_full: () => free() < capacity * 0.25,
  };
}
