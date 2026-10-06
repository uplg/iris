import type { Output } from 'mediabunny';

/** Mediabunny's MP4 muxer validates that every packet's PTS is ≥ the
 *  max PTS of the previous GOP. That assumption breaks for open-GOP
 *  / deep B-frame video (x265, AV1 with `--b-pyramid normal`, anything
 *  exported by HandBrake with a tight RD), where a new GOP's keyframe
 *  legitimately presents 1 frame before the previous GOP's last
 *  B-frame. The muxer's per-sample PTS/CTS book-keeping handles this
 *  fine, so the only fix needed is to swallow the "previous GOP" error
 *  thrown by the validator. We patch the validator on each Output we
 *  build (the muxer is on `output._muxer`). */
export function relaxMediabunnyGopCheck(output: Output): void {
	const m = (
		output as unknown as {
			_muxer?: { validateTimestamp?: (track: unknown, ts: number, isKey: boolean) => void };
		}
	)._muxer;
	if (!m || typeof m.validateTimestamp !== 'function') return;
	const original = m.validateTimestamp.bind(m);
	m.validateTimestamp = (track, ts, isKey) => {
		try {
			original(track, ts, isKey);
		} catch (e) {
			if (e instanceof Error && /previous GOP/i.test(e.message)) return;
			throw e;
		}
	};
}
