// Latency baseline for the main user actions (sprint 9). Plain Node 22, no dependencies.
//
//   node tools/perf/bench.mjs --write-limits .data/bench-limits.yaml
//     Writes a copy of config/limits.yaml with every quota multiplied by 10 000, so a bench run is not
//     throttled while each request still pays the same rate-limit store round trips. Start the API
//     with API_LIMITS_CONFIG_PATH pointing at it (local only; never in a shared environment).
//
//   node tools/perf/bench.mjs [--base http://127.0.0.1:8080] [--samples 200] [--concurrency 1]
//                             [--only feed,card.open] [--json out.json]
//     Needs a seeded local stack (`pnpm dev:seed`) and an API with API_DEV_SIGN_IN=true: it signs in
//     as the seed users through /v1/dev/sign-in, then times each action and prints p50/p95/p99/max.
//
// Each sample is one HTTP request timed on the client (loopback, keep-alive), so the numbers are the
// server time plus a few hundred microseconds of local transport. Writes (send, react) create real
// rows in the local database; run it against a disposable local stack only.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { deflateSync } from 'node:zlib';
import { randomUUID } from 'node:crypto';

const args = parseArgs(process.argv.slice(2));

if (args['write-limits']) {
  const source = readFileSync(args.limits ?? 'config/limits.yaml', 'utf8');
  const relaxed = source.replace(
    /^(\s*(?:requests|new_account_requests):\s*)(\d+)/gm,
    (_, key, n) => `${key}${Number(n) * 10_000}`,
  ).replace(/\{\s*requests:\s*(\d+)/g, (_, n) => `{ requests: ${Number(n) * 10_000}`);
  mkdirSync(dirname(args['write-limits']), { recursive: true });
  writeFileSync(args['write-limits'], relaxed);
  console.log(`wrote ${args['write-limits']}`);
  process.exit(0);
}

const BASE = (args.base ?? 'http://127.0.0.1:8080').replace(/\/$/, '');
const SAMPLES = Number(args.samples ?? 200);
const CONCURRENCY = Number(args.concurrency ?? 1);
const WARMUP = 10;
const only = args.only ? new Set(String(args.only).split(',')) : null;

async function call(method, path, { token, body, headers = {}, raw } = {}) {
  const init = { method, headers: { Accept: 'application/json', ...headers } };
  if (token) init.headers.Authorization = `Bearer ${token}`;
  if (raw) {
    init.body = raw;
  } else if (body !== undefined) {
    init.headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const started = process.hrtime.bigint();
  const response = await fetch(`${BASE}${path}`, init);
  const text = await response.text();
  const ms = Number(process.hrtime.bigint() - started) / 1e6;
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  return { status: response.status, json, ms };
}

async function signIn(handle) {
  const r = await call('POST', '/v1/dev/sign-in', { body: { handle } });
  if (r.status !== 200) throw new Error(`sign-in ${handle}: ${r.status} ${JSON.stringify(r.json)}`);
  return r.json;
}

/** A tiny solid-colour RGBA PNG (zlib from Node), so the bench needs no image library. */
function png(width, height, rgba) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const rows = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) rows.set(rgba, y * (width * 4 + 1) + 1 + x * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(rows)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

async function upload(token, kind, bytes) {
  const r = await call('POST', `/v1/media?kind=${kind}`, {
    token,
    raw: bytes,
    headers: { 'Content-Type': 'image/png' },
  });
  if (r.status !== 200 && r.status !== 201) throw new Error(`upload ${kind}: ${r.status} ${JSON.stringify(r.json)}`);
  return r.json.media.id;
}

function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  const index = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, index)];
}

async function measure(name, fn) {
  if (only && !only.has(name)) return null;
  for (let i = 0; i < WARMUP; i++) await fn(i);
  const times = [];
  const statuses = new Map();
  let next = 0;
  async function worker() {
    while (next < SAMPLES) {
      const i = next++;
      const r = await fn(i + WARMUP);
      times.push(r.ms);
      statuses.set(r.status, (statuses.get(r.status) ?? 0) + 1);
    }
  }
  const wall = process.hrtime.bigint();
  await Promise.all(Array.from({ length: CONCURRENCY }, worker));
  const seconds = Number(process.hrtime.bigint() - wall) / 1e9;
  times.sort((a, b) => a - b);
  const row = {
    action: name,
    n: times.length,
    p50: percentile(times, 50),
    p95: percentile(times, 95),
    p99: percentile(times, 99),
    max: times.at(-1),
    rps: times.length / seconds,
    statuses: Object.fromEntries(statuses),
  };
  console.log(
    `${name.padEnd(16)} n=${String(row.n).padStart(4)} p50=${row.p50.toFixed(1).padStart(7)} ms` +
      ` p95=${row.p95.toFixed(1).padStart(7)} ms p99=${row.p99.toFixed(1).padStart(7)} ms` +
      ` max=${row.max.toFixed(1).padStart(7)} ms  ${row.rps.toFixed(0)} req/s  status=${JSON.stringify(row.statuses)}`,
  );
  return row;
}

const alice = await signIn('alice.demo');
const bob = await signIn('bob.demo');
const feed = await call('GET', '/v1/feed', { token: bob.access_token });
const cardId = feed.json?.items?.[0]?.id;
const boards = await call('GET', '/v1/boards', { token: bob.access_token });
const boardId = boards.json?.items?.[0]?.id;
if (!cardId || !boardId) throw new Error('seed data missing: run `pnpm dev:seed` first');
const me = await call('GET', '/v1/me', { token: bob.access_token });
const bobId = me.json?.account?.id ?? me.json?.id;

console.log(`base=${BASE} samples=${SAMPLES} concurrency=${CONCURRENCY} warmup=${WARMUP}`);
const rows = [];
const push = (row) => row && rows.push(row);

push(await measure('sign-in', () => call('POST', '/v1/dev/sign-in', { body: { handle: 'bob.demo' } })));
let refreshToken = (await signIn('bob.demo')).refresh_token;
push(
  await measure('refresh', async () => {
    const r = await call('POST', '/v1/auth/refresh', { body: { refresh_token: refreshToken } });
    if (r.json?.refresh_token) refreshToken = r.json.refresh_token;
    return r;
  }),
);
push(await measure('me', () => call('GET', '/v1/me', { token: bob.access_token })));
push(await measure('feed', () => call('GET', '/v1/feed', { token: bob.access_token })));
push(await measure('card.open', () => call('GET', `/v1/cards/${cardId}`, { token: bob.access_token })));
push(await measure('boards.list', () => call('GET', '/v1/boards', { token: bob.access_token })));
push(await measure('board.load', () => call('GET', `/v1/boards/${boardId}`, { token: bob.access_token })));
const kinds = ['react.heart', 'react.squish'];
push(
  await measure('react', (i) =>
    call('PUT', `/v1/cards/${cardId}/reaction`, { token: bob.access_token, body: { kind: kinds[i % 2] } }),
  ),
);

if (!only || only.has('card.send')) {
  const render = await upload(alice.access_token, 'card_render', png(64, 64, [240, 120, 60, 255]));
  const mask = await upload(alice.access_token, 'mask', png(64, 64, [255, 255, 255, 255]));
  push(
    await measure('card.send', () => {
      const id = randomUUID();
      return call('POST', '/v1/cards', {
        token: alice.access_token,
        headers: { 'Idempotency-Key': id },
        body: {
          client_card_id: id,
          render_media_id: render,
          mask_media_id: mask,
          style: { cut: { id: 'cut.diecut', version: 1, params: { borderWidthPct: 2.5 } } },
          caption: 'bench',
          audience: { mode: 'subset', recipient_ids: [bobId] },
        },
      });
    }),
  );
}

if (args.json) writeFileSync(args.json, JSON.stringify({ base: BASE, samples: SAMPLES, concurrency: CONCURRENCY, rows }, null, 2));

function parseArgs(list) {
  const out = {};
  for (let i = 0; i < list.length; i++) {
    const key = list[i].replace(/^--/, '');
    const value = list[i + 1] && !list[i + 1].startsWith('--') ? list[++i] : true;
    out[key] = value;
  }
  return out;
}
