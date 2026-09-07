/* A dumb static host, the way Cloudflare Pages or GitHub Pages serves the
   shop: files off disk and a 404 for everything else. No panda is started, so
   /health genuinely goes unanswered and the page has to reach that conclusion
   the way it will in production — not because a query string told it to. */
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(process.env.PANDA_ROOT || path.resolve(here, "../.."), "static");
const port = Number(process.env.PW_PAGES_PORT || 8804);

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  // The engine is only streamed straight into WebAssembly when the host says
  // what it is; a wrong type here is exactly how a real deploy breaks.
  ".wasm": "application/wasm",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
};

createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  const rel = url.pathname === "/" ? "/index.html" : url.pathname;
  const file = path.join(root, path.normalize(rel));
  // Nothing above the published directory is reachable, as on a real host.
  if (!file.startsWith(root)) {
    res.statusCode = 403;
    res.end("no");
    return;
  }
  let stat;
  try {
    stat = statSync(file);
  } catch {
    stat = null;
  }
  if (!stat || !stat.isFile()) {
    // /health lands here. A static host has no cafe behind it and says so.
    res.statusCode = 404;
    res.setHeader("content-type", "text/plain; charset=utf-8");
    res.end("404");
    return;
  }
  res.statusCode = 200;
  res.setHeader("content-type", TYPES[path.extname(file)] || "application/octet-stream");
  res.setHeader("content-length", stat.size);
  createReadStream(file).pipe(res);
}).listen(port, "127.0.0.1", () => {
  console.log(`static host on http://127.0.0.1:${port} serving ${root}`);
});
