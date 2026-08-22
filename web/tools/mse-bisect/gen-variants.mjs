// Produit un fMP4 fragmenté démarrant à un keyframe MI-FLUX, à partir d'un MKV
// local. C'est le fichier que `zen-check.html` découpe ensuite en variantes.
//
//   bun gen-variants.mjs /chemin/vers/film.mkv 180
//
import {
  Input,
  Output,
  Mp4OutputFormat,
  StreamTarget,
  FilePathSource,
  EncodedPacketSink,
  EncodedVideoPacketSource,
  ALL_FORMATS,
} from "mediabunny";
import { writeFileSync } from "node:fs";

const [file, seekArg] = process.argv.slice(2);
if (!file) {
  console.error("usage: bun gen-variants.mjs <film.mkv> [secondes]");
  process.exit(1);
}
const seek = Number(seekArg ?? 180);

const input = new Input({ source: new FilePathSource(file), formats: ALL_FORMATS });
const parts = [];
const output = new Output({
  format: new Mp4OutputFormat({ fastStart: "fragmented", minimumFragmentDuration: 1 }),
  target: new StreamTarget(
    new WritableStream({
      write(c) {
        parts.push({ pos: c.position, data: new Uint8Array(c.data) });
      },
    }),
  ),
});
const track = await input.getPrimaryVideoTrack();
const src = new EncodedVideoPacketSource(await track.getCodec());
output.addVideoTrack(src);
const sink = new EncodedPacketSink(track);
const start = await sink.getKeyPacket(seek);
await output.start();
const config = await track.getDecoderConfig();
let first = true;
for await (const p of sink.packets(start)) {
  if (p.timestamp > start.timestamp + 10) break;
  if (p.timestamp < start.timestamp) continue; // images de tête, comme Tier B
  await src.add(p, first ? { decoderConfig: config } : undefined);
  first = false;
}
await src.close();
await output.finalize();
const total = parts.reduce((m, p) => Math.max(m, p.pos + p.data.length), 0);
const buf = new Uint8Array(total);
for (const p of parts) buf.set(p.data, p.pos);
writeFileSync("midstream.mp4", buf);
console.log(`midstream.mp4 écrit — keyframe à ${start.timestamp.toFixed(2)}s`);
await input.dispose();
