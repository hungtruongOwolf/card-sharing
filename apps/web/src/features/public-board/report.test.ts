import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { FAKE_CAPTCHA_ANSWER, captchaProvider, captchaVerifier } from '../captcha/captcha.ts';
import type { ServiceApiConfig } from '../service-api/service-api.ts';

import { OUTCOME_STATUS, isSameOriginJson, parseReportRequest, submitReport } from './report.ts';

const TOKEN = 'AbCdEfGhIjKlMnOpQrSt_-';
const CREDENTIAL = 'c'.repeat(32);
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

describe('captcha port', () => {
  it('is off unless configured, and the fake never runs in production', () => {
    assert.equal(captchaProvider({}), null);
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'turnstile' }), null);
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'fake', NODE_ENV: 'development' }), 'fake');
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'fake', NODE_ENV: 'production' }), null);
  });

  it('fake verifier accepts only its own answer', async () => {
    const verifier = captchaVerifier('fake');
    assert.equal(await verifier.verify(FAKE_CAPTCHA_ANSWER, '1.1.1.1'), true);
    assert.equal(await verifier.verify('anything', '1.1.1.1'), false);
    assert.equal(await verifier.verify('', '1.1.1.1'), false);
  });

  it('widget answer matches the server constant', () => {
    const widget = readFileSync(new URL('../captcha/CaptchaField.tsx', import.meta.url), 'utf8');
    assert.ok(widget.includes(`'${FAKE_CAPTCHA_ANSWER}'`));
  });
});

describe('parseReportRequest', () => {
  it('accepts an AC-26.1 reason, an optional note and a captcha answer', () => {
    assert.deepEqual(parseReportRequest({ reason: 'spam', captcha: 'x' }), {
      reason: 'spam',
      note: null,
      captcha: 'x',
    });
    assert.deepEqual(parseReportRequest({ reason: 'other', note: '  hi  ', captcha: 'x' }), {
      reason: 'other',
      note: 'hi',
      captcha: 'x',
    });
  });

  it('refuses unknown reasons, long notes and junk', () => {
    assert.equal(parseReportRequest({ reason: 'nudity', captcha: 'x' }), null);
    assert.equal(parseReportRequest({ reason: 'spam', note: 'x'.repeat(501), captcha: 'x' }), null);
    assert.equal(parseReportRequest({ reason: 'spam', note: 5, captcha: 'x' }), null);
    assert.equal(parseReportRequest({ reason: 'spam', captcha: 'x'.repeat(5000) }), null);
    assert.equal(parseReportRequest([]), null);
    assert.equal(parseReportRequest('spam'), null);
  });

  it('tells a missing captcha apart (400 captcha)', () => {
    assert.equal(parseReportRequest({ reason: 'spam' }), 'no-captcha');
    assert.equal(parseReportRequest({ reason: 'spam', captcha: '' }), 'no-captcha');
    assert.equal(OUTCOME_STATUS.captcha, 400);
    assert.equal(OUTCOME_STATUS.limited, 429);
  });
});

describe('isSameOriginJson', () => {
  const site = 'https://cards.example';
  it('needs this site as Origin and a JSON body', () => {
    const ok = new Headers({ origin: site, 'content-type': 'application/json; charset=utf-8' });
    assert.equal(isSameOriginJson(ok, site), true);
    for (const headers of [
      new Headers({ origin: 'https://evil.example', 'content-type': 'application/json' }),
      new Headers({ 'content-type': 'application/json' }),
      new Headers({ origin: site, 'content-type': 'application/x-www-form-urlencoded' }),
      new Headers({ origin: site, 'content-type': 'text/plain' }),
    ]) {
      assert.equal(isSameOriginJson(headers, site), false);
    }
  });
});

describe('submitReport', () => {
  it('posts the report with the service credential and the visitor IP', async () => {
    const api = fakeFetch(() => new Response(null, { status: 202 }));
    const outcome = await submitReport(
      TOKEN,
      { reason: 'hate', note: 'see card 2' },
      CONFIG,
      '198.51.100.4',
      api.fetch,
    );
    assert.equal(outcome, 'accepted');
    const call = api.calls[0];
    assert.equal(call?.url, 'http://api.internal:8080/v1/public/reports');
    assert.equal(call?.init?.method, 'POST');
    const headers = new Headers(call?.init?.headers);
    assert.equal(headers.get('x-service-credential'), CREDENTIAL);
    assert.equal(headers.get('x-client-ip'), '198.51.100.4');
    assert.deepEqual(JSON.parse(String(call?.init?.body)), {
      token: TOKEN,
      reason: 'hate',
      note: 'see card 2',
    });
  });

  it('never sends the captcha answer or a malformed token to the API', async () => {
    const api = fakeFetch(() => new Response(null, { status: 202 }));
    assert.equal(
      await submitReport('bad', { reason: 'spam', note: null }, CONFIG, '1.1.1.1', api.fetch),
      'gone',
    );
    assert.equal(api.calls.length, 0);
    await submitReport(TOKEN, { reason: 'spam', note: null }, CONFIG, '1.1.1.1', api.fetch);
    assert.deepEqual(JSON.parse(String(api.calls[0]?.init?.body)), {
      token: TOKEN,
      reason: 'spam',
    });
  });

  it('maps API answers to outcomes', async () => {
    const cases: [number, string][] = [
      [201, 'accepted'],
      [204, 'accepted'],
      [400, 'invalid'],
      [404, 'gone'],
      [429, 'limited'],
      [401, 'unavailable'],
      [503, 'unavailable'],
    ];
    for (const [status, outcome] of cases) {
      const api = fakeFetch(() => new Response(null, { status }));
      assert.equal(
        await submitReport(TOKEN, { reason: 'spam', note: null }, CONFIG, '1.1.1.1', api.fetch),
        outcome,
      );
    }
    const down = fakeFetch(() => Promise.reject(new Error('down')));
    assert.equal(
      await submitReport(TOKEN, { reason: 'spam', note: null }, CONFIG, '1.1.1.1', down.fetch),
      'unavailable',
    );
  });
});

describe('report route source', () => {
  const route = readFileSync(
    new URL('../../app/b/[token]/report/route.ts', import.meta.url),
    'utf8',
  );

  it('verifies the captcha before calling the API and never logs', () => {
    assert.ok(route.indexOf('.verify(') < route.indexOf('submitReport(token'));
    assert.equal(/console\.(log|info|warn|error|debug)/.test(route), false);
  });
});
