/**
 * Offline check for `hevc-cra-splice.ts` against a real file.
 *
 *   bun tools/hevc-splice/check.ts <file.mkv|mp4> <seekSeconds> [runSeconds=8]
 *
 * Reads the run Tier B would feed after a seek (the key packet at/before
 * `seekSeconds`, RASL leading pictures dropped), writes it twice as Annex B —
 * untouched and spliced — and decodes both with ffmpeg. The splice is right
 * when ffmpeg reports no error on the spliced stream and every decoded frame
 * has the same CRC in both, in the same order. Needs `ffmpeg` on PATH.
 */

import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { ALL_FORMATS, EncodedPacketSink, FilePathSource, Input } from "mediabunny";

import { HevcCraSplicer, descriptionBytes } from "../../src/lib/iris-core/decode/hevc-cra-splice";

const [file, seekArg, runArg] = process.argv.slice(2);
if (!file || !seekArg) {
  console.error("usage: bun tools/hevc-splice/check.ts <file> <seekSeconds> [runSeconds]");
  process.exit(2);
}
const seek = Number(seekArg);
const runSeconds = runArg ? Number(runArg) : 8;

const input = new Input({ source: new FilePathSource(file), formats: ALL_FORMATS });
const track = await input.getPrimaryVideoTrack();
if (!track) throw new Error("no video track");
const codec = await track.getCodec();
if (codec !== "hevc") throw new Error(`video codec is ${codec}, not hevc`);
const config = await track.getDecoderConfig();
const hvcc = descriptionBytes(config?.description ?? undefined);
const nalLengthSize = ((hvcc[21] as number) & 3) + 1;

const START = Uint8Array.of(0, 0, 0, 1);
function annexB(chunks: Uint8Array[], accessUnit: Uint8Array): void {
  let p = 0;
  while (p + nalLengthSize <= accessUnit.length) {
    let len = 0;
    for (let i = 0; i < nalLengthSize; i += 1) len = (len << 8) | (accessUnit[p + i] as number);
    p += nalLengthSize;
    chunks.push(START, accessUnit.subarray(p, p + len));
    p += len;
  }
}

/** VPS/SPS/PPS out of the hvcC arrays, so the raw stream is self-describing. */
function parameterSets(): Uint8Array[] {
  const out: Uint8Array[] = [];
  let p = 23;
  const numArrays = hvcc[22] as number;
  for (let a = 0; a < numArrays; a += 1) {
    const count = ((hvcc[p + 1] as number) << 8) | (hvcc[p + 2] as number);
    p += 3;
    for (let i = 0; i < count; i += 1) {
      const len = ((hvcc[p] as number) << 8) | (hvcc[p + 1] as number);
      p += 2;
      out.push(START, hvcc.subarray(p, p + len));
      p += len;
    }
  }
  return out;
}

function nalTypesOf(accessUnit: Uint8Array): number[] {
  const types: number[] = [];
  let p = 0;
  while (p + nalLengthSize <= accessUnit.length) {
    let len = 0;
    for (let i = 0; i < nalLengthSize; i += 1) len = (len << 8) | (accessUnit[p + i] as number);
    p += nalLengthSize;
    types.push(((accessUnit[p] as number) >> 1) & 0x3f);
    p += len;
  }
  return types;
}

const sink = new EncodedPacketSink(track);
const start = (await sink.getKeyPacket(seek)) ?? (await sink.getFirstKeyPacket());
if (!start) throw new Error("no key packet");
console.log(
  `run opens at t=${start.timestamp.toFixed(3)} NALs=[${nalTypesOf(start.data).join(",")}]`,
);

const splicer = new HevcCraSplicer(hvcc);
const original: Uint8Array[] = parameterSets();
const spliced: Uint8Array[] = parameterSets();
let packets = 0;
let rewritten = 0;
for await (const packet of sink.packets(start)) {
  if (packet.timestamp > start.timestamp + runSeconds) break;
  if (packet.timestamp < start.timestamp) continue; // RASL, as Tier B drops them
  annexB(original, packet.data);
  const out = splicer.transform(packet.data);
  if (out.data !== packet.data) rewritten += 1;
  if (packets === 0) console.log(`first AU after splice NALs=[${nalTypesOf(out.data).join(",")}]`);
  annexB(spliced, out.data);
  packets += 1;
}
console.log(`${packets} access units, ${rewritten} rewritten`);

const dir = mkdtempSync(join(tmpdir(), "hevc-splice-"));
const write = (name: string, chunks: Uint8Array[]): string => {
  const path = join(dir, name);
  writeFileSync(path, Buffer.concat(chunks.map((c) => Buffer.from(c))));
  return path;
};
const origPath = write("original.h265", original);
const splicedPath = write("spliced.h265", spliced);
console.log(`wrote ${origPath} and ${splicedPath}`);

function decode(path: string): { crc: string[]; stderr: string } {
  const res = spawnSync(
    "ffmpeg",
    ["-hide_banner", "-v", "error", "-i", path, "-f", "framecrc", "-"],
    { encoding: "utf8", maxBuffer: 1 << 28 },
  );
  const crc = res.stdout
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.split(",").slice(4).join(",").trim()); // size + CRC only
  return { crc, stderr: res.stderr.trim() };
}

const a = decode(origPath);
const b = decode(splicedPath);
console.log(`original: ${a.crc.length} frames${a.stderr ? `\n  ffmpeg: ${a.stderr}` : ""}`);
console.log(`spliced:  ${b.crc.length} frames${b.stderr ? `\n  ffmpeg: ${b.stderr}` : ""}`);
let ok = b.stderr === "" && a.crc.length === b.crc.length && a.crc.length > 0;
for (let i = 0; ok && i < a.crc.length; i += 1) {
  if (a.crc[i] !== b.crc[i]) {
    console.log(`frame ${i} differs: ${a.crc[i]} vs ${b.crc[i]}`);
    ok = false;
  }
}
console.log(ok ? "OK: identical decode, no decoder error" : "FAIL");
process.exit(ok ? 0 : 1);
