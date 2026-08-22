import { createServer } from "node:http";
import { readFileSync, statSync, createReadStream } from "node:fs";
const LATENCY = Number(process.env.LATENCY_MS ?? 0);
const MKV = "/Users/leonard/Github/iris/waking-bug-seek.mkv";
const WEB = "/Users/leonard/Github/iris/web";
const MB = WEB + "/node_modules/mediabunny/dist/bundles/mediabunny.mjs";
createServer((req, res) => {
  const url = req.url.split("?")[0];
  if (url === "/media.mkv") {
    const st = statSync(MKV);
    const range = req.headers.range;
    if (range) {
      const m = /bytes=(\d+)-(\d*)/.exec(range);
      const start = Number(m[1]),
        end = m[2] ? Number(m[2]) : st.size - 1;
      res.writeHead(206, {
        "content-range": `bytes ${start}-${end}/${st.size}`,
        "accept-ranges": "bytes",
        "content-length": end - start + 1,
        "content-type": "video/x-matroska",
      });
      const send = () => createReadStream(MKV, { start, end }).pipe(res);
      // Latence artificielle par requête de plage : LATENCY_MS=120 node serve.mjs
      if (LATENCY) setTimeout(send, LATENCY);
      else send();
      return;
    }
    res.writeHead(200, { "content-length": st.size, "accept-ranges": "bytes" });
    createReadStream(MKV).pipe(res);
    return;
  }
  let p;
  if (url === "/mb/x.js") p = MB;
  else if (url === "/mb/patched.js") p = "./mb-patched.js";
  else if (url.startsWith("/libavjs-pkg/")) p = WEB + "/node_modules/libav.js/" + url.slice(13);
  else if (url.startsWith("/libavjs/")) {
    try {
      readFileSync("." + url);
      p = "." + url;
    } catch {
      p = WEB + "/public" + url;
    }
  } else p = url === "/" ? "page.html" : url.slice(1);
  try {
    const b = readFileSync(p);
    const ct = p.endsWith(".html")
      ? "text/html"
      : p.endsWith(".wasm")
        ? "application/wasm"
        : "text/javascript";
    res.writeHead(200, { "content-type": ct });
    res.end(b);
  } catch {
    res.writeHead(404);
    res.end();
  }
}).listen(8099, () => console.log("ready"));
