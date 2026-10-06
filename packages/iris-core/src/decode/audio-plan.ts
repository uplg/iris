/**
 * How an audio track reaches an fMP4 for MSE: passed through when MSE plays its codec, or
 * decoded (WebCodecs or the registered libav.js decoder) and re-encoded with the browser's
 * `AudioEncoder`. Shared by every engine that muxes for MSE (B, B live, C live, E).
 */

import { AudioSampleSource, Quality, type InputAudioTrack } from 'mediabunny';

import { ensureLibavAudioDecoderRegistered } from './libav-audio-decoder';
import { libavCanDecode } from './libav-codecs';

/** Codecs MSE plays inside fMP4 without help: passthrough, no re-encode. */
export const MSE_NATIVE_AUDIO: ReadonlySet<string> = new Set(['aac', 'opus', 'mp3']);

/** Result of probing `WebCodecs.AudioEncoder`: which target codec
 *  works at what channel count for the given source. Returns null
 *  when neither AAC nor Opus encoding works (caller fails the
 *  mount and demotes to F).
 *
 *  We probe in priority order:
 *    1. **AAC** — broadest device / receiver compat. Chrome accepts
 *       up to 5.1ch + the `format: 'aac'` field Mediabunny requires
 *       for AAC-in-MP4. Firefox doesn't support AAC-in-MP4 encoding
 *       at all (its WebCodecs AudioEncoder only emits ADTS).
 *    2. **Opus** — Firefox's fallback. 2ch only (browser Opus
 *       encoders are practically stereo-capped). MSE-in-MP4 accepts
 *       `audio/mp4; codecs="opus"` on Chrome + Firefox since ~2020.
 *
 *  Critical: the AAC probe MUST pass `aac: { format: 'aac' }` to
 *  match Mediabunny's own internal config. Firefox returns
 *  `supported: true` on the bare query and then rejects the encoder
 *  once `format` is set — probing without `format` would green-
 *  light Tier B on Firefox and we'd waste a full mount cycle. */
export type AudioEncoderChoice =
	| { codec: 'aac'; channels: number; mp4Codec: 'mp4a.40.2' }
	| { codec: 'opus'; channels: number; mp4Codec: 'opus' };

const encoderProbeCache = new Map<string, AudioEncoderChoice | null>();

export async function pickAudioEncoder(srcChannels: number, sampleRate: number): Promise<AudioEncoderChoice | null> {
	if (typeof globalThis.AudioEncoder === 'undefined') return null;
	const key = `${srcChannels}/${sampleRate}`;
	const cached = encoderProbeCache.get(key);
	if (cached !== undefined) return cached;

	// Pass 1 — AAC at descending channel counts (prefer source layout).
	const aacCandidates = Array.from(new Set([srcChannels, 6, 2].filter((n) => n > 0 && n <= srcChannels)));
	for (const n of aacCandidates) {
		try {
			const r = await AudioEncoder.isConfigSupported({
				codec: 'mp4a.40.2',
				sampleRate,
				numberOfChannels: n,
				bitrate: 192_000,
				aac: { format: 'aac' }
			} as AudioEncoderConfig);
			if (r.supported) {
				const choice: AudioEncoderChoice = {
					codec: 'aac',
					channels: n,
					mp4Codec: 'mp4a.40.2'
				};
				console.log(`[iris-core] AudioEncoder → AAC ${n}ch @ ${sampleRate}Hz (source: ${srcChannels}ch)`);
				encoderProbeCache.set(key, choice);
				return choice;
			}
		} catch {
			/* keep walking */
		}
	}

	// Pass 2 — Opus 2ch (Firefox fallback). 128 kbps is around the
	// transparency point for music; speech-heavy content sounds fine
	// well below that, so this is conservative.
	try {
		const r = await AudioEncoder.isConfigSupported({
			codec: 'opus',
			sampleRate,
			numberOfChannels: 2,
			bitrate: 128_000,
			opus: { format: 'opus' }
		} as AudioEncoderConfig);
		if (r.supported) {
			const choice: AudioEncoderChoice = {
				codec: 'opus',
				channels: 2,
				mp4Codec: 'opus'
			};
			console.log(`[iris-core] AudioEncoder → Opus 2ch @ ${sampleRate}Hz (source: ${srcChannels}ch, AAC unavailable)`);
			encoderProbeCache.set(key, choice);
			return choice;
		}
	} catch {
		/* fall through */
	}

	console.warn(`[iris-core] no encodable audio codec @ ${sampleRate}Hz (source: ${srcChannels}ch)`);
	encoderProbeCache.set(key, null);
	return null;
}

/** The encoder bitrate for a target codec. */
export function encoderBitrate(codec: 'aac' | 'opus'): number {
	return codec === 'opus' ? 128_000 : 192_000;
}

/** Mediabunny's encode-on-add source for a transcoded track, downmixing when the encoder
 *  takes fewer channels than the source carries. */
export function transcodeSampleSource(choice: { codec: 'aac' | 'opus'; channels: number }, srcChannels: number): AudioSampleSource {
	return new AudioSampleSource({
		codec: choice.codec,
		// `new Quality(<number>)` means a 0..1 qualitative level, NOT
		// a bitrate — the explicit `{ bitrate }` form is required.
		quality: new Quality({ bitrate: encoderBitrate(choice.codec) }),
		...(choice.channels !== srcChannels ? { transform: { numberOfChannels: choice.channels } } : {})
	});
}

export type AudioPlan =
	| { kind: 'passthrough'; mp4Codec: string }
	| { kind: 'transcode'; mp4Codec: string; targetCodec: 'aac' | 'opus'; channels: number };

/** The plan for a demuxed track (the live engines read it from the stream, VOD from the
 *  manifest): null when the codec can't be played at all (the caller goes video only). Throws
 *  when the codec is decodable but this browser can't re-encode it. */
export async function planAudioTrack(track: InputAudioTrack, label: string): Promise<AudioPlan | null> {
	const codec = await track.getCodec();
	if (!codec) return null;
	if (MSE_NATIVE_AUDIO.has(codec)) {
		const cfg = await track.getDecoderConfig();
		return { kind: 'passthrough', mp4Codec: cfg?.codec ?? 'mp4a.40.2' };
	}
	if (!libavCanDecode(codec)) {
		console.warn(`[iris-core] ${label}: audio codec ${codec} undecodable — video only`);
		return null;
	}
	ensureLibavAudioDecoderRegistered();
	const channels = await track.getNumberOfChannels();
	const sampleRate = await track.getSampleRate();
	const choice = await pickAudioEncoder(channels, sampleRate);
	if (!choice) throw new Error(`${label}: cannot re-encode ${codec} in this browser`);
	return { kind: 'transcode', mp4Codec: choice.mp4Codec, targetCodec: choice.codec, channels: choice.channels };
}
