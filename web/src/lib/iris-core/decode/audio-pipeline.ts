/**
 * Mediabunny demux → `AudioDecoder` → `AudioData` callback stream.
 *
 * Mirror of `video-pipeline.ts` for audio tracks. Caller owns the
 * `AudioData` and MUST `close()` it once consumed (or queued onto an
 * `AudioBuffer`).
 */

import { EncodedPacketSink, type InputAudioTrack } from "mediabunny";

export type AudioPipelineOptions = {
  track: InputAudioTrack;
  config: AudioDecoderConfig;
  startSeconds?: number;
  onData: (data: AudioData) => void;
  onError: (err: Error) => void;
  onEnd?: () => void;
};

export type AudioPipelineHandle = {
  stop: () => Promise<void>;
};

export function startAudioPipeline(opts: AudioPipelineOptions): AudioPipelineHandle {
  const decoder = new AudioDecoder({
    output: (data) => {
      try {
        opts.onData(data);
      } catch (e) {
        try {
          data.close();
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
      /* benign */
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
        opts.onError(new Error("Tier C: no decodable audio packet found"));
        return;
      }
      for await (const packet of sink.packets(startPacket)) {
        if (stopped) break;
        while (decoder.decodeQueueSize > 32 && !stopped) {
          await new Promise<void>((r) => setTimeout(r, 4));
        }
        if (stopped) break;
        decoder.decode(packet.toEncodedAudioChunk());
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
