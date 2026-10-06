// Arithmetic on a SourceBuffer's `buffered` ranges, shared by every MSE engine. Pure: anything
// with `length`, `start(i)` and `end(i)` will do (tests pass plain arrays through `ranges`).

export type Ranges = Pick<TimeRanges, 'length' | 'start' | 'end'>;

/** A `Ranges` over `[start, end]` pairs. */
export function ranges(pairs: ReadonlyArray<readonly [number, number]>): Ranges {
	return {
		length: pairs.length,
		start: (i) => pairs[i]![0],
		end: (i) => pairs[i]![1]
	};
}

/** Seconds of media buffered after `t`. CRITICAL: walks forward across ADJACENT ranges,
 *  bridging the sub-second gaps between fMP4 fragments that fail to coalesce into one
 *  `buffered` range (a SourceBuffer can hold 100 s+ in a dozen touching ranges; counting only
 *  the first one let the back-pressure under-count wildly and the buffer exhaust memory). A
 *  genuine hole wider than `bridge` ends the count: the playhead can't cross it anyway. */
export function bufferedAhead(b: Ranges, t: number, bridge = 2): number {
	let coveredEnd = Number.NEGATIVE_INFINITY;
	for (let i = 0; i < b.length; i += 1) {
		const start = b.start(i);
		const end = b.end(i);
		if (coveredEnd === Number.NEGATIVE_INFINITY) {
			// the first range that covers (or sits just after) the playhead
			if (start <= t + 0.5 && end >= t) coveredEnd = end;
		} else if (start - coveredEnd <= bridge) {
			coveredEnd = end;
		} else {
			break;
		}
	}
	return coveredEnd === Number.NEGATIVE_INFINITY ? 0 : Math.max(0, coveredEnd - t);
}

/** Seconds left in the one range holding `t` (no bridging), 0 when `t` isn't buffered. */
export function aheadInRange(b: Ranges, t: number, slack = 0.25): number {
	for (let i = 0; i < b.length; i += 1) {
		if (b.start(i) - slack <= t && b.end(i) + slack >= t) return Math.max(0, b.end(i) - t);
	}
	return 0;
}

/** Whether `t` sits in a buffered range, give or take `slack`. */
export function coversTime(b: Ranges, t: number, slack = 0.25): boolean {
	for (let i = 0; i < b.length; i += 1) {
		if (b.start(i) - slack <= t && b.end(i) + slack >= t) return true;
	}
	return false;
}

/** Whether data is buffered from just before `t` onwards: where a deferred playhead may land. */
export function landsAt(b: Ranges, t: number): boolean {
	for (let i = 0; i < b.length; i += 1) {
		if (b.start(i) - 0.25 <= t && b.end(i) >= t) return true;
	}
	return false;
}

/** The played-out span to `remove()`: from the first buffered start up to `keep` seconds
 *  behind `t`. Null until that span is at least `minSpan` long, so a 4 Hz `timeupdate` doesn't
 *  turn into four SourceBuffer operations a second, each delaying the next append. */
export function evictionSpan(b: Ranges, t: number, keep: number, minSpan = 0): [number, number] | null {
	const before = t - keep;
	if (before <= 0 || b.length === 0) return null;
	const first = b.start(0);
	if (first >= before || before - first < minSpan) return null;
	return [first, before];
}

/** The buffered spans outside the window `[t - behind, t + ahead]`, to `remove()` after a seek:
 *  the ranges a seek leaves behind (or ahead) sit outside what the engine's own trimming walks,
 *  which follows the playhead. Spans of a second or less are left. */
export function outsideWindow(b: Ranges, t: number, behind: number, ahead: number): [number, number][] {
	if (b.length === 0) return [];
	const out: [number, number][] = [];
	const first = b.start(0);
	const last = b.end(b.length - 1);
	if (t - behind - first > 1) out.push([first, t - behind]);
	if (last - (t + ahead) > 1) out.push([Math.max(first, t + ahead), last]);
	return out;
}

/** Where to jump over a small forward hole (an evicted or discarded fragment): the start of
 *  the next real range within `maxJump` of `t`. Zero-width ranges (Firefox leaves them behind
 *  after a `remove()`) don't count. */
export function forwardGapTarget(b: Ranges, t: number, maxJump = 8): number | null {
	for (let i = 0; i < b.length; i += 1) {
		const start = b.start(i);
		if (b.end(i) - start < 0.05) continue;
		if (start > t && start - t < maxJump) return start;
	}
	return null;
}

/** Total seconds buffered. */
export function bufferedSpan(b: Ranges): number {
	let span = 0;
	for (let i = 0; i < b.length; i += 1) span += b.end(i) - b.start(i);
	return span;
}

/** `0-12 30-45`, for the diagnostics. */
export function describeRanges(b: Ranges): string {
	if (b.length === 0) return 'empty';
	const parts: string[] = [];
	for (let i = 0; i < b.length; i += 1) parts.push(`${b.start(i).toFixed(0)}-${b.end(i).toFixed(0)}`);
	return parts.join(' ');
}
