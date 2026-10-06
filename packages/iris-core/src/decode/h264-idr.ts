// Telling a true H.264 IDR from an open-GOP recovery point, on the packets mediabunny hands out:
// length-prefixed (AVCC, from MP4 / Matroska, with an avcC description) or Annex B start codes
// (MPEG-TS, no description).

/** NAL length-prefix size from the avcC description; 0 without one (Annex B). */
export function nalLengthSize(description: BufferSource | undefined): number {
	if (!description) return 0;
	const bytes =
		description instanceof ArrayBuffer
			? new Uint8Array(description)
			: new Uint8Array(description.buffer, description.byteOffset, description.byteLength);
	// avcC: [0]=version [1]=profile [2]=compat [3]=level [4]=0xFC|lengthSizeMinusOne
	if (bytes.length < 5 || bytes[0] !== 1) return 4;
	return (bytes[4]! & 0x03) + 1;
}

/** True when the sample contains an IDR slice (NAL type 5). Broadcast TNT mostly emits
 *  open-GOP recovery-point I-frames — containers mark them as sync samples, but starting a
 *  strict decoder (VideoToolbox) on one is an illegal random access and it hard-fails
 *  instantly. Only a real IDR (fresh DPB) is a safe anchor. `lengthSize` 0 = Annex B. */
export function packetHasIdr(data: Uint8Array, lengthSize: number): boolean {
	if (lengthSize === 0) {
		for (let i = 0; i + 3 < data.length; i += 1) {
			if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1 && (data[i + 3]! & 0x1f) === 5) return true;
		}
		return false;
	}
	let o = 0;
	while (o + lengthSize < data.length) {
		let len = 0;
		for (let i = 0; i < lengthSize; i += 1) len = (len << 8) | data[o + i]!;
		o += lengthSize;
		if (len <= 0 || o + len > data.length) break;
		if ((data[o]! & 0x1f) === 5) return true;
		o += len;
	}
	return false;
}
