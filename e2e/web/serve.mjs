// Static server for the Expo web export (apps/mobile/dist) that the browser E2E suite drives in CI.
//
// - Single-page app (app.json `web.output: "single"`): any path that is not a file gets index.html, so deep links
//   and reloads reach the app's router.
// - The export is a development build (`expo export --dev`: dev sign-in, plain-http API). Expo's dev clients open
//   WebSockets to /hot and /message on the page origin (Metro's HMR and dev commands); the handshake is accepted and
//   the socket held open with frames ignored, so the browser logs no connection errors for a server that is not Metro.
// - Loopback only, no caching, no directory listing, no path traversal.
import { createHash } from 'node:crypto';
import { createReadStream, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
const root = resolve(here, process.env.E2E_APP_DIST ?? '../../apps/mobile/dist');
const port = Number(process.env.E2E_APP_PORT ?? 8081);
const host = '127.0.0.1';
const WEBSOCKET_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.map': 'application/json',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

/** The file under `root` for a URL path, or null (missing, a directory, or outside the root). */
function fileFor(pathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const candidate = normalize(join(root, decoded));
  if (candidate !== root && !candidate.startsWith(root + sep)) return null;
  try {
    return statSync(candidate).isFile() ? candidate : null;
  } catch {
    return null;
  }
}

const server = createServer((request, response) => {
  const { pathname } = new URL(request.url ?? '/', `http://${host}`);
  let file = fileFor(pathname);
  if (!file && !extname(pathname)) file = join(root, 'index.html');
  if (!file || (request.method !== 'GET' && request.method !== 'HEAD')) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, {
    'content-type': TYPES[extname(file).toLowerCase()] ?? 'application/octet-stream',
    'cache-control': 'no-store',
  });
  if (request.method === 'HEAD') {
    response.end();
    return;
  }
  createReadStream(file).pipe(response);
});

server.on('upgrade', (request, socket) => {
  const key = request.headers['sec-websocket-key'];
  if (typeof key !== 'string') {
    socket.destroy();
    return;
  }
  const accept = createHash('sha1')
    .update(key + WEBSOCKET_GUID)
    .digest('base64');
  socket.write(
    [
      'HTTP/1.1 101 Switching Protocols',
      'Upgrade: websocket',
      'Connection: Upgrade',
      `Sec-WebSocket-Accept: ${accept}`,
      '',
      '',
    ].join('\r\n'),
  );
  socket.on('data', () => {});
  socket.on('error', () => {});
});

server.listen(port, host, () => {
  console.log(`Serving ${root} on http://${host}:${port}`);
});
