// Top-level ISOBMFF box walking over the muxer's output chunks.

export type Box = { type: string; start: number; end: number };

/** The complete top-level boxes at the head of `buf`, in order. Stops at the first box that is
 *  truncated (still being written) or malformed. */
export function topLevelBoxes(buf: Uint8Array): Box[] {
	const boxes: Box[] = [];
	const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
	let o = 0;
	while (o + 8 <= buf.byteLength) {
		let size = dv.getUint32(o);
		const type = String.fromCharCode(buf[o + 4]!, buf[o + 5]!, buf[o + 6]!, buf[o + 7]!);
		if (size === 1) {
			if (o + 16 > buf.byteLength) break;
			size = Number(dv.getBigUint64(o + 8));
		}
		if (size < 8 || o + size > buf.byteLength) break;
		boxes.push({ type, start: o, end: o + size });
		o += size;
	}
	return boxes;
}

/** If `buf` holds an init segment (`ftyp`/`moov`) immediately followed by a media segment
 *  (`moof`), the two halves; otherwise null. */
export function initSegmentSplit(buf: Uint8Array): [Uint8Array, Uint8Array] | null {
	let sawInit = false;
	for (const box of topLevelBoxes(buf)) {
		if (box.type === 'ftyp' || box.type === 'moov') sawInit = true;
		else if (box.type === 'moof') return sawInit && box.start > 0 ? [buf.subarray(0, box.start), buf.subarray(box.start)] : null;
		else return null;
	}
	return null;
}

/** Byte offset just past `moov`, or -1 while it is still incomplete. */
export function initSegmentEnd(buf: Uint8Array): number {
	return topLevelBoxes(buf).find((b) => b.type === 'moov')?.end ?? -1;
}
