/**
 * Mediabunny demux → `VideoDecoder` → `VideoFrame` callback stream.
 *
 * The pipeline reads encoded video packets from a Mediabunny input track,
 * feeds them to a configured `VideoDecoder`, and surfaces decoded
 * `VideoFrame`s to the caller via the `onFrame` callback. The caller
 * owns each frame and MUST call `frame.close()` once it has been
 * rendered — leaking frames pins GPU memory and stalls the decoder.
 *
 * Backpressure: the pipeline awaits `decoder.decodeQueueSize <= 8`
 * between decode calls so the worker thread doesn't grow an unbounded
 * frame queue.
 */

import { EncodedPacketSink, type InputVideoTrack } from "mediabunny";

export type VideoPipelineOptions = {
  track: InputVideoTrack;
  config: VideoDecoderConfig;
  /** Optional starting timestamp in seconds (snaps to the previous key frame). */
  startSeconds?: number;
  onFrame: (frame: VideoFrame) => void;
  onError: (err: Error) => void;
  /** Fired when the input track is fully decoded (after `flush()`). */
  onEnd?: () => void;
};

export type VideoPipelineHandle = {
  stop: () => Promise<void>;
};

export function startVideoPipeline(opts: VideoPipelineOptions): VideoPipelineHandle {
  const decoder = new VideoDecoder({
    output: (frame) => {
      try {
        opts.onFrame(frame);
      } catch (e) {
        // The caller's frame handler threw — close the frame to keep the
        // decoder healthy and propagate so the player can fail loudly.
        try {
          frame.close();
        } catch {
          /* idempotent */
        }
        opts.onError(e instanceof Error ? e : new Error(String(e)));
      }
    },
    error: (err) => opts.onError(err),
  });

  let stopped = false;
  const stop = async (): Promise<void> => {
    if (stopped) return;
    stopped = true;
    try {
      await decoder.flush();
    } catch {
      /* flush may reject when there were no in-flight chunks; benign */
    }
    try {
      decoder.close();
    } catch {
      /* idempotent */
    }
  };

  void (async () => {
    try {
      decoder.configure(opts.config);
      const sink = new EncodedPacketSink(opts.track);
      const startPacket =
        opts.startSeconds && opts.startSeconds > 0
          ? await sink.getKeyPacket(opts.startSeconds)
          : await sink.getFirstKeyPacket();
      if (!startPacket) {
        opts.onError(new Error("Tier C: no decodable packet found"));
        return;
      }
      for await (const packet of sink.packets(startPacket)) {
        if (stopped) break;
        // Soft backpressure: don't let the decoder's internal queue grow
        // without bound. 8 outstanding packets keeps Chrome's GPU sched
        // pipelined without burning RAM.
        while (decoder.decodeQueueSize > 8 && !stopped) {
          await new Promise<void>((r) => setTimeout(r, 4));
        }
        if (stopped) break;
        decoder.decode(packet.toEncodedVideoChunk());
      }
      if (!stopped) {
        try {
          await decoder.flush();
        } catch {
          /* benign */
        }
        opts.onEnd?.();
      }
    } catch (e) {
      if (!stopped) opts.onError(e instanceof Error ? e : new Error(String(e)));
    }
  })();

  return { stop };
}
