import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import type { ServiceApiConfig } from '../service-api/service-api.ts';

import {
  isBoardToken,
  lookupPublicBoard,
  lookupPublicBoardOg,
  mediaOrigin,
  parseBoard,
  pinStylesheet,
} from './public-board.ts';

const TOKEN = 'AbCdEfGhIjKlMnOpQrSt_-';
const CREDENTIAL = 'c'.repeat(32);
const MEDIA = 'https://media.example';
const TAG = `pl-${'0123456789abcdef'.repeat(2)}`;
const CONFIG: ServiceApiConfig = {
  apiUrl: new URL('http://api.internal:8080'),
  credential: CREDENTIAL,
  clientIpHeader: 'fly-client-ip',
};

type Call = { url: string; init: RequestInit | undefined };

function fakeFetch(respond: () => Response | Promise<Response>) {
  const calls: Call[] = [];
  const impl = (async (input: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return respond();
  }) as typeof fetch;
  return { fetch: impl, calls };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

const CARD = {
  kind: 'card',
  x: 0.5,
  y: 0.25,
  scale: 1,
  rotation: 10,
  z_key: 'a0',
  image_url: `${MEDIA}/public/abc.webp?exp=1&v=x&sig=y`,
  image_expires_at: '2026-11-06T10:00:00Z',
  width: 600,
  height: 840,
};

function body(overrides: Record<string, unknown> = {}) {
  return {
    board: { title: 'Weekend scrapbook', backdrop: null, owner_display_name: 'Alice' },
    pins: [
      CARD,
      { kind: 'private_tile', x: 0.1, y: 0.1, scale: 1, rotation: 0, z_key: 'a1' },
      { kind: 'decoration', x: 0.2, y: 0.2, scale: 1, rotation: 0, z_key: 'a2', decoration: {} },
    ],
    noindex: true,
    cache_ttl_s: 60,
    cache_tag: TAG,
    ...overrides,
  };
}

describe('isBoardToken', () => {
  it('accepts only 22 base64url characters', () => {
    assert.equal(isBoardToken(TOKEN), true);
    assert.equal(isBoardToken(TOKEN.slice(1)), false);
    assert.equal(isBoardToken(`${TOKEN}A`), false);
    assert.equal(isBoardToken('../../v1/me/aaaaaaaaaaa'), false);
    assert.equal(isBoardToken('AbCdEfGhIjKlMnOpQrSt%2'), false);
  });
});

describe('mediaOrigin', () => {
  it('prefers MEDIA_ORIGIN and falls back to the API origin', () => {
    assert.equal(mediaOrigin({ MEDIA_ORIGIN: 'https://cdn.example/x' }), 'https://cdn.example');
    assert.equal(mediaOrigin({ API_URL: 'http://127.0.0.1:8080' }), 'http://127.0.0.1:8080');
    assert.equal(mediaOrigin({ MEDIA_ORIGIN: 'javascript:alert(1)' }), null);
    assert.equal(mediaOrigin({}), null);
  });
});

describe('lookupPublicBoard', () => {
  it('calls the public route with the service credential and the visitor IP', async () => {
    const api = fakeFetch(() => json(200, body()));
    const result = await lookupPublicBoard(TOKEN, CONFIG, '198.51.100.4', MEDIA, api.fetch);
    assert.equal(result.kind, 'found');
    assert.equal(api.calls.length, 1);
    const call = api.calls[0];
    assert.equal(call?.url, `http://api.internal:8080/v1/public/boards/${TOKEN}`);
    const headers = new Headers(call?.init?.headers);
    assert.equal(headers.get('x-service-credential'), CREDENTIAL);
    assert.equal(headers.get('x-client-ip'), '198.51.100.4');
    assert.equal(call?.init?.cache, 'no-store');
    assert.equal(call?.init?.redirect, 'error');
  });

  it('answers a malformed token as gone without calling the API', async () => {
    const api = fakeFetch(() => json(200, body()));
    assert.deepEqual(await lookupPublicBoard('short', CONFIG, '1.1.1.1', MEDIA, api.fetch), {
      kind: 'gone',
    });
    assert.equal(api.calls.length, 0);
  });

  it('maps 404 to gone and every other failure to unavailable', async () => {
    const gone = fakeFetch(() => json(404, { code: 'NOT_FOUND' }));
    assert.deepEqual(await lookupPublicBoard(TOKEN, CONFIG, '1.1.1.1', MEDIA, gone.fetch), {
      kind: 'gone',
    });
    for (const respond of [
      () => json(429, { code: 'RATE_LIMITED' }),
      () => json(503, { code: 'TEMPORARILY_UNAVAILABLE' }),
      () => json(401, { code: 'UNAUTHENTICATED' }),
      () => json(200, body({ cache_tag: 'pl-x' })),
      () => json(200, body({ pins: 'nope' })),
      () => new Response('not json', { status: 200 }),
      () => Promise.reject(new Error(`connect failed for /v1/public/boards/${TOKEN}`)),
    ]) {
      const api = fakeFetch(respond);
      assert.deepEqual(await lookupPublicBoard(TOKEN, CONFIG, '1.1.1.1', MEDIA, api.fetch), {
        kind: 'unavailable',
      });
    }
  });
});

describe('parseBoard', () => {
  it('keeps cards, makes private tiles neutral and drops decorations', () => {
    const board = parseBoard(body(), MEDIA);
    assert.ok(board);
    assert.equal(board.title, 'Weekend scrapbook');
    assert.equal(board.ownerDisplayName, 'Alice');
    assert.equal(board.cacheTag, TAG);
    assert.equal(board.pins.length, 2);
    assert.equal(board.pins[0]?.kind, 'card');
    // A private tile carries its position only: no id, URL or size.
    assert.deepEqual(board.pins[1], {
      kind: 'private_tile',
      x: 0.1,
      y: 0.1,
      scale: 1,
      rotation: 0,
    });
  });

  it('never keeps ids the API might add', () => {
    const board = parseBoard(
      body({ pins: [{ ...CARD, card_id: 'secret', owner_id: 'secret', id: 'secret' }] }),
      MEDIA,
    );
    assert.equal(JSON.stringify(board).includes('secret'), false);
  });

  it('turns a card with an image from another origin into a neutral tile', () => {
    for (const image_url of [
      'https://evil.example/a.webp',
      'javascript:alert(1)',
      'data:image/png;base64,AAAA',
      '/relative.webp',
      42,
    ]) {
      const board = parseBoard(body({ pins: [{ ...CARD, image_url }] }), MEDIA);
      assert.equal(board?.pins[0]?.kind, 'private_tile');
    }
    assert.equal(parseBoard(body(), null)?.pins[0]?.kind, 'private_tile');
  });

  it('clamps layout numbers and drops pins without them', () => {
    const board = parseBoard(
      body({
        pins: [
          { ...CARD, x: 7, y: -3, scale: 1000, rotation: -9999 },
          { ...CARD, x: 'NaN' },
        ],
      }),
      MEDIA,
    );
    assert.equal(board?.pins.length, 1);
    const pin = board?.pins[0];
    assert.deepEqual([pin?.x, pin?.y, pin?.scale, pin?.rotation], [1, 0, 10, -360]);
  });

  it('keeps names and titles as plain data (React escapes them when rendering)', () => {
    const payload = '<img src=x onerror=alert(1)>';
    const board = parseBoard(
      body({ board: { title: payload, owner_display_name: payload } }),
      MEDIA,
    );
    assert.equal(board?.title, payload);
    assert.equal(board?.ownerDisplayName, payload);
  });

  it('caps the number of pins', () => {
    const board = parseBoard(body({ pins: Array.from({ length: 150 }, () => CARD) }), MEDIA);
    assert.equal(board?.pins.length, 100);
  });
});

describe('lookupPublicBoardOg', () => {
  it('returns the signed URL and cache tag', async () => {
    const url = `${MEDIA}/og/abc.png?exp=1&v=x&sig=y`;
    const api = fakeFetch(() =>
      json(200, { url, expires_at: '2026-11-06T10:00:00Z', cache_ttl_s: 60, cache_tag: TAG }),
    );
    assert.deepEqual(await lookupPublicBoardOg(TOKEN, CONFIG, '1.1.1.1', MEDIA, api.fetch), {
      kind: 'found',
      url,
      cacheTag: TAG,
    });
    assert.equal(api.calls[0]?.url, `http://api.internal:8080/v1/public/boards/${TOKEN}/og`);
  });

  it('refuses a redirect target outside the media origin', async () => {
    const api = fakeFetch(() =>
      json(200, { url: 'https://evil.example/x', cache_ttl_s: 60, cache_tag: TAG }),
    );
    assert.deepEqual(await lookupPublicBoardOg(TOKEN, CONFIG, '1.1.1.1', MEDIA, api.fetch), {
      kind: 'unavailable',
    });
  });

  it('maps 404 and malformed tokens to gone', async () => {
    const api = fakeFetch(() => json(404, {}));
    assert.deepEqual(await lookupPublicBoardOg(TOKEN, CONFIG, '1.1.1.1', MEDIA, api.fetch), {
      kind: 'gone',
    });
    assert.deepEqual(await lookupPublicBoardOg('x', CONFIG, '1.1.1.1', MEDIA, api.fetch), {
      kind: 'gone',
    });
    assert.equal(api.calls.length, 1);
  });
});

describe('pinStylesheet', () => {
  it('places pins with numbers only', () => {
    const css = pinStylesheet([{ x: 0.5, y: 0.25, scale: 2, rotation: 12.345 }]);
    assert.equal(
      css,
      '.pin-0{left:50%;top:25%;width:20%;height:26.667%;' +
        'transform:translate(-50%,-50%) rotate(12.35deg);z-index:1}',
    );
    assert.doesNotMatch(css, /[<>"'\\]/);
  });
});

describe('board page source', () => {
  const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
  const sources = [
    read('../../app/b/[token]/page.tsx'),
    read('../../app/b/[token]/not-found.tsx'),
    read('../../app/b/[token]/og/route.ts'),
  ];

  it('renders API strings only as text and never logs', () => {
    for (const source of sources) {
      assert.equal(source.includes('dangerouslySetInnerHTML'), false);
      assert.equal(/console\.(log|info|warn|error|debug)/.test(source), false);
    }
  });

  it('keeps board pages out of search results', () => {
    assert.match(sources[0] ?? '', /index: false/);
  });
});
