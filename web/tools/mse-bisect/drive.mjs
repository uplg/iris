import { firefox } from "playwright";

// Usage: node drive.mjs ["?src=…&at=…"]
const query = process.argv[2] ?? "";
const b = await firefox.launch();
const p = await b.newPage();
p.on("console", (m) => console.log(m.text()));
p.on("pageerror", (e) => console.log("PAGEERROR " + e.message));
await p.goto("http://127.0.0.1:8099/" + query);
await p.waitForFunction(() => document.title === "done", null, { timeout: 300000 }).catch(() => {});
await b.close();
