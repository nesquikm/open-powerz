/**
 * A tiny static server for the Pages suite (FR QFTCX1, AC-QFTCX1.4): it mounts a built
 * folder at `/open-powerz/`, as GitHub Pages serves the project site, and answers 404
 * everywhere else, so an asset URL that ignores the base path fails the suite.
 *
 *   bun e2e/pages/serve.ts [dir=dist-pages] [port=4174]
 */

import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve, sep } from "node:path";

const BASE = "/open-powerz/";
const root = resolve(process.argv[2] ?? "dist-pages");
const port = Number(process.argv[3] ?? 4174);

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
};

createServer((req, res) => {
  const path = decodeURIComponent((req.url ?? "/").split(/[?#]/)[0] ?? "/");
  if (path === BASE.slice(0, -1)) {
    res.writeHead(301, { location: BASE }).end();
    return;
  }
  if (!path.startsWith(BASE)) {
    res.writeHead(404).end("not under /open-powerz/");
    return;
  }
  let file = normalize(join(root, path.slice(BASE.length)));
  if (file !== root && !file.startsWith(root + sep)) {
    res.writeHead(403).end();
    return;
  }
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, "index.html");
  if (!existsSync(file)) {
    res.writeHead(404).end("not found");
    return;
  }
  res.writeHead(200, {
    "content-type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream",
  });
  createReadStream(file).pipe(res);
}).listen(port, "127.0.0.1", () => {
  console.log(`serving ${root} at http://127.0.0.1:${port}${BASE}`);
});
