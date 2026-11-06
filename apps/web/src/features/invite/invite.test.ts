import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import {
  type InviteApiConfig,
  UNKNOWN_VISITOR_IP,
  isInviteToken,
  lookupInvite,
  readInviteApiConfig,
  visitorIp,
} from './invite.ts';

const TOKEN = 'AbCdEfGhIjKlMnOpQrSt_-';
const CREDENTIAL = 'c'.repeat(32);
const CONFIG: InviteApiConfig = {
  apiUrl: new URL('http://api.internal:8080'),
  credential: CREDENTIAL,
  clientIpHeader: 'fly-client-ip',
};

type Call = { url: string; init: RequestInit | undefined };

function fakeFetch(respond: () => Response | Promise<Response>): {
  fetch: typeof fetch;
  calls: Call[];
} {
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

describe('isInviteToken', () => {
  it('accepts only 22 base64url characters', () => {
    assert.equal(isInviteToken(TOKEN), true);
    assert.equal(isInviteToken(TOKEN.slice(1)), false);
    assert.equal(isInviteToken(`${TOKEN}A`), false);
    assert.equal(isInviteToken('AbCdEfGhIjKlMnOpQrSt%2'), false);
    assert.equal(isInviteToken('../../v1/me/invite/xxx'), false);
  });
});

describe('readInviteApiConfig', () => {
  it('needs an API URL and a long credential', () => {
    assert.equal(readInviteApiConfig({}), null);
    assert.equal(
      readInviteApiConfig({ API_URL: 'http://api', API_SERVICE_CREDENTIAL: 'short' }),
      null,
    );
    assert.equal(
      readInviteApiConfig({ API_URL: 'not a url', API_SERVICE_CREDENTIAL: CREDENTIAL }),
      null,
    );
    assert.equal(
      readInviteApiConfig({ API_URL: 'file:///etc/passwd', API_SERVICE_CREDENTIAL: CREDENTIAL }),
      null,
    );
    const config = readInviteApiConfig({
      API_URL: 'https://api.example',
      API_SERVICE_CREDENTIAL: CREDENTIAL,
      CLIENT_IP_HEADER: 'Fly-Client-IP',
    });
    assert.equal(config?.apiUrl.origin, 'https://api.example');
    assert.equal(config?.clientIpHeader, 'fly-client-ip');
  });
});

describe('visitorIp', () => {
  it('reads only the configured edge header', () => {
    const headers = new Headers({ 'fly-client-ip': '198.51.100.4', 'x-forwarded-for': '1.2.3.4' });
    assert.equal(visitorIp(headers, 'fly-client-ip'), '198.51.100.4');
    // Without a trusted header a forged X-Forwarded-For is ignored: one shared bucket.
    assert.equal(visitorIp(headers, null), UNKNOWN_VISITOR_IP);
    assert.equal(
      visitorIp(new Headers({ 'fly-client-ip': 'nope' }), 'fly-client-ip'),
      UNKNOWN_VISITOR_IP,
    );
    assert.equal(
      visitorIp(new Headers({ 'fly-client-ip': '2001:db8::1' }), 'fly-client-ip'),
      '2001:db8::1',
    );
  });
});

describe('lookupInvite', () => {
  it('calls the public route with the service credential and the visitor IP', async () => {
    const api = fakeFetch(() => json(200, { inviter: { display_name: 'Mai' } }));
    const result = await lookupInvite(TOKEN, CONFIG, '198.51.100.4', api.fetch);
    assert.deepEqual(result, { kind: 'found', displayName: 'Mai' });
    assert.equal(api.calls.length, 1);
    const call = api.calls[0];
    assert.equal(call?.url, `http://api.internal:8080/v1/public/invites/${TOKEN}`);
    const headers = new Headers(call?.init?.headers);
    assert.equal(headers.get('x-service-credential'), CREDENTIAL);
    assert.equal(headers.get('x-client-ip'), '198.51.100.4');
    assert.equal(headers.get('authorization'), null);
    assert.equal(call?.init?.cache, 'no-store');
    assert.equal(call?.init?.redirect, 'error');
  });

  it('answers a malformed token as gone without calling the API', async () => {
    const api = fakeFetch(() => json(200, { inviter: { display_name: 'Mai' } }));
    assert.deepEqual(await lookupInvite('short', CONFIG, '1.1.1.1', api.fetch), { kind: 'gone' });
    assert.equal(api.calls.length, 0);
  });

  it('maps every 404 to the one neutral state', async () => {
    const api = fakeFetch(() => json(404, { code: 'NOT_FOUND' }));
    assert.deepEqual(await lookupInvite(TOKEN, CONFIG, '1.1.1.1', api.fetch), { kind: 'gone' });
  });

  it('reports other failures as unavailable, never as a verdict on the token', async () => {
    for (const respond of [
      () => json(429, { code: 'RATE_LIMITED' }),
      () => json(503, { code: 'TEMPORARILY_UNAVAILABLE' }),
      () => json(401, { code: 'UNAUTHENTICATED' }),
      () => json(200, { inviter: { display_name: 42 } }),
      () => json(200, { inviter: { display_name: '' } }),
      () => json(200, { inviter: { display_name: 'x'.repeat(481) } }),
      () => new Response('not json', { status: 200 }),
      () => Promise.reject(new Error(`connect failed for /v1/public/invites/${TOKEN}`)),
    ]) {
      const api = fakeFetch(respond);
      assert.deepEqual(await lookupInvite(TOKEN, CONFIG, '1.1.1.1', api.fetch), {
        kind: 'unavailable',
      });
    }
  });

  it('returns a display name as plain data (React escapes it when rendering)', async () => {
    const name = '<script>alert(1)</script>';
    const api = fakeFetch(() => json(200, { inviter: { display_name: name } }));
    assert.deepEqual(await lookupInvite(TOKEN, CONFIG, '1.1.1.1', api.fetch), {
      kind: 'found',
      displayName: name,
    });
  });
});

describe('invite page source', () => {
  const page = readFileSync(new URL('../../app/i/[token]/page.tsx', import.meta.url), 'utf8');

  it('renders the name only as text and never logs', () => {
    assert.equal(page.includes('dangerouslySetInnerHTML'), false);
    assert.equal(/console\.(log|info|warn|error|debug)/.test(page), false);
  });

  it('keeps invite pages out of search results', () => {
    assert.match(page, /index: false/);
  });
});
