/**
 * Mediabunny demux → `AudioDecoder` → `AudioData` callback stream.
 *
 * Mirror of `video-pipeline.ts` for audio tracks. Caller owns the
 * `AudioData` and MUST `close()` it once consumed (or queued onto an
 * `AudioBuffer`).
 */

import { AudioSampleSink, EncodedPacketSink, type InputAudioTrack } from 'mediabunny';

import { ensureLibavAudioDecoderRegistered } from './libav-audio-decoder';
import { libavCanDecode } from './libav-codecs';
import { DECODE_QUEUE_POLL_MS, PACING_POLL_MS } from './video-pipeline';
import { configWithFreshDescription } from './webcodecs-probe';

export type AudioPipelineOptions = {
	track: InputAudioTrack;
	config: AudioDecoderConfig;
	startSeconds?: number;
	onData: (data: AudioData) => void;
	onError: (err: Error) => void;
	onEnd?: () => void;
	/** Whether the packet at this timestamp (seconds) may be decoded now: the ring the
	 *  scheduler plays from holds a few seconds, so the decode is paced on its clock. */
	canDecode?: (timestampSeconds: number) => boolean;
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
		error: (err) => opts.onError(err)
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
			const codec = await opts.track.getCodec();
			if (codec && libavCanDecode(codec) && !(await nativelyDecodable(opts.config))) {
				await decodeThroughLibav(opts, () => stopped);
				return;
			}
			decoder.configure(configWithFreshDescription(opts.config) as AudioDecoderConfig);
			const sink = new EncodedPacketSink(opts.track);
			const startPacket =
				opts.startSeconds && opts.startSeconds > 0 ? await sink.getKeyPacket(opts.startSeconds) : await sink.getFirstKeyPacket();
			if (!startPacket) {
				opts.onError(new Error('Tier C: no decodable audio packet found'));
				return;
			}
			for await (const packet of sink.packets(startPacket)) {
				if (stopped) break;
				while (decoder.decodeQueueSize > 32 && !stopped) {
					await new Promise<void>((r) => setTimeout(r, DECODE_QUEUE_POLL_MS));
				}
				while (opts.canDecode && !opts.canDecode(packet.timestamp) && !stopped) {
					await new Promise<void>((r) => setTimeout(r, PACING_POLL_MS));
				}
				if (stopped) break;
				try {
					decoder.decode(packet.toEncodedAudioChunk());
				} catch (e) {
					// The decoder was closed between the queue-size check and
					// this call (race with `stop()`). Treat as benign — the
					// caller's stop happens because something else already
					// surfaced the real error.
					if (stopped) break;
					const msg = e instanceof Error ? e.message : String(e);
					if (msg.includes('closed codec')) break;
					throw e;
				}
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

async function nativelyDecodable(config: AudioDecoderConfig): Promise<boolean> {
	try {
		return (await AudioDecoder.isConfigSupported(configWithFreshDescription(config) as AudioDecoderConfig)).supported === true;
	} catch {
		return false;
	}
}

/** A codec WebCodecs refuses (E-AC-3, AC-3, FLAC… in most browsers): mediabunny decodes it
 *  through the registered libav.js decoder, same pacing, same `AudioData` out. */
async function decodeThroughLibav(opts: AudioPipelineOptions, stopped: () => boolean): Promise<void> {
	ensureLibavAudioDecoderRegistered();
	const sink = new AudioSampleSink(opts.track);
	for await (const sample of sink.samples(opts.startSeconds && opts.startSeconds > 0 ? opts.startSeconds : 0)) {
		while (opts.canDecode && !opts.canDecode(sample.timestamp) && !stopped()) {
			await new Promise<void>((r) => setTimeout(r, PACING_POLL_MS));
		}
		if (stopped()) {
			sample.close();
			break;
		}
		const data = sample.toAudioData();
		sample.close();
		try {
			opts.onData(data);
		} catch (e) {
			data.close();
			throw e;
		}
	}
	if (!stopped()) opts.onEnd?.();
}
