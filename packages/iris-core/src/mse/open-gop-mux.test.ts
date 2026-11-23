import { BufferTarget, EncodedPacket, EncodedVideoPacketSource, Mp4OutputFormat, Output } from 'mediabunny';
import { describe, expect, it } from 'vitest';
import { relaxMediabunnyGopCheck } from './output.ts';

// Mediabunny asserted on these until 1.66.3 and Iris carried a patch for it: an open GOP whose
// leading pictures present before the previous GOP's last ones. Each row is a GOP in decode
// order (frame numbers as PTS), its first entry the keyframe.
const FPS = 25;
const STREAMS: Record<string, number[][]> = {
	'leading pictures before the previous GOP ends': [
		[0, 4, 2, 1, 3, 8, 6, 5],
		[10, 7, 9, 14, 12, 11, 13],
		[17, 15, 16, 20, 19, 18]
	],
	'a deep overlap': [
		[0, 8, 4, 2, 1, 3, 6, 5, 7, 12, 10],
		[16, 9, 11, 13, 14, 15, 20, 18, 17, 19]
	]
};

async function mux(gops: number[][], fastStart: 'fragmented' | 'in-memory'): Promise<number> {
	const target = new BufferTarget();
	const output = new Output({ format: new Mp4OutputFormat({ fastStart, minimumFragmentDuration: 0.1 }), target });
	relaxMediabunnyGopCheck(output);
	const source = new EncodedVideoPacketSource('vp9');
	output.addVideoTrack(source, { frameRate: FPS });
	await output.start();
	let first = true;
	for (const gop of gops) {
		for (const [i, frame] of gop.entries()) {
			const packet = new EncodedPacket(new Uint8Array(64), i === 0 ? 'key' : 'delta', frame / FPS, 1 / FPS);
			const meta = first ? { decoderConfig: { codec: 'vp09.00.10.08', codedWidth: 320, codedHeight: 240 } } : undefined;
			await source.add(packet, meta);
			first = false;
		}
	}
	await output.finalize();
	return target.buffer?.byteLength ?? 0;
}

describe('open-GOP video through the unpatched Mediabunny muxer', () => {
	for (const [name, gops] of Object.entries(STREAMS)) {
		for (const fastStart of ['fragmented', 'in-memory'] as const) {
			it(`muxes ${name} (${fastStart})`, async () => {
				expect(await mux(gops, fastStart)).toBeGreaterThan(0);
			});
		}
	}
});
