/* Iris ring-buffer AudioWorklet processor.
 * Vendored as plain JS to sidestep Vite's TS/worker bundling rules.
 * Pairs with `web/src/lib/iris-core/audio/ring-buffer.ts`.
 *
 * SAB layout:
 *   Int32 header at offset 0: [read_idx, write_idx, channels, capacity]
 *   Float32 payload after that — interleaved samples, ring-indexed.
 */

const HEADER_SIZE = 4;
const READ_IDX = 0;
const WRITE_IDX = 1;

class IrisRingProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.samples = null;
    this.header = null;
    this.channels = 0;
    this.capacity = 0;
    this.port.onmessage = (e) => {
      const m = e.data;
      if (m && m.type === "init") {
        this.header = new Int32Array(m.sab, 0, HEADER_SIZE);
        this.samples = new Float32Array(m.sab, HEADER_SIZE * 4);
        this.channels = m.channels;
        this.capacity = m.capacity;
      }
    };
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || !this.samples || !this.header) return true;
    const frames = output[0] ? output[0].length : 0;
    const outCh = output.length;
    if (!frames) return true;
    const needed = frames * this.channels;
    let read = Atomics.load(this.header, READ_IDX);
    const write = Atomics.load(this.header, WRITE_IDX);
    let available = write - read;
    if (available < 0) available += this.capacity;
    if (available < needed) {
      // Underrun → silence (one click rather than a stall).
      for (let c = 0; c < outCh; c += 1) output[c].fill(0);
      return true;
    }
    for (let i = 0; i < frames; i += 1) {
      for (let c = 0; c < outCh; c += 1) {
        const sourceCh = c < this.channels ? c : 0;
        const idx = (read + sourceCh) % this.capacity;
        output[c][i] = this.samples[idx];
      }
      read = (read + this.channels) % this.capacity;
    }
    Atomics.store(this.header, READ_IDX, read);
    return true;
  }
}

registerProcessor("iris-ring", IrisRingProcessor);
