// A minimal PGS (.sup) encoder for the bench: ffmpeg can mux a PGS track but not draw one.
// Writes one bitmap cue (a framed white bar) shown for the first second of every even second
// from 0 to 58, the same timing as the ASS fixture. Usage: bun pgs.ts out.sup
import { writeFileSync } from 'node:fs';

const W = 640;
const H = 360;
const BAR_W = 240;
const BAR_H = 48;
const BAR_X = (W - BAR_W) / 2;
const BAR_Y = H - BAR_H - 30;

function segment(type: number, pts90k: number, body: number[]): number[] {
	const b = [0x50, 0x47];
	for (const v of [pts90k, 0]) b.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff);
	b.push(type, (body.length >> 8) & 0xff, body.length & 0xff, ...body);
	return b;
}

const u16 = (v: number) => [(v >> 8) & 0xff, v & 0xff];

function pcs(composition: number, show: boolean): number[] {
	const body = [...u16(W), ...u16(H), 0x10, ...u16(composition), show ? 0x80 : 0x00, 0x00, 0x00, show ? 1 : 0];
	if (show) body.push(...u16(0), 0, 0, ...u16(BAR_X), ...u16(BAR_Y));
	return body;
}

const wds = () => [1, 0, ...u16(BAR_X), ...u16(BAR_Y), ...u16(BAR_W), ...u16(BAR_H)];

// entry 1 opaque white, 2 opaque black (Y, Cr, Cb, A)
const pds = () => [0, 0, 1, 235, 128, 128, 255, 2, 16, 128, 128, 255];

function run(len: number, color: number): number[] {
	if (color === 0) return len < 64 ? [0, len] : [0, 0x40 | (len >> 8), len & 0xff];
	if (len < 3) return Array.from({ length: len }, () => color);
	return len < 64 ? [0, 0x80 | len, color] : [0, 0xc0 | (len >> 8), len & 0xff, color];
}

function ods(): number[] {
	const rle: number[] = [];
	for (let y = 0; y < BAR_H; y++) {
		const border = y < 4 || y >= BAR_H - 4;
		if (border) rle.push(...run(BAR_W, 2));
		else rle.push(...run(4, 2), ...run(BAR_W - 8, 1), ...run(4, 2));
		rle.push(0, 0);
	}
	const len = rle.length + 4;
	return [...u16(0), 0, 0xc0, (len >> 16) & 0xff, (len >> 8) & 0xff, len & 0xff, ...u16(BAR_W), ...u16(BAR_H), ...rle];
}

const out: number[] = [];
let composition = 0;
for (let s = 0; s <= 58; s += 2) {
	const on = s * 90_000;
	const off = (s + 1) * 90_000;
	out.push(...segment(0x16, on, pcs(composition++, true)), ...segment(0x17, on, wds()));
	out.push(...segment(0x14, on, pds()), ...segment(0x15, on, ods()), ...segment(0x80, on, []));
	out.push(...segment(0x16, off, pcs(composition++, false)), ...segment(0x17, off, wds()), ...segment(0x80, off, []));
}
const path = process.argv[2];
if (!path) throw new Error('usage: bun pgs.ts out.sup');
writeFileSync(path, Uint8Array.from(out));
