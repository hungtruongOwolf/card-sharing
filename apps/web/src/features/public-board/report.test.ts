import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { FAKE_CAPTCHA_TOKEN, captchaProvider } from '../captcha/captcha.ts';
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

describe('captcha provider', () => {
  it('is off unless configured, and the fake never runs in production', () => {
    assert.equal(captchaProvider({}), null);
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'turnstile' }), null);
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'fake', NODE_ENV: 'development' }), 'fake');
    assert.equal(captchaProvider({ CAPTCHA_PROVIDER: 'fake', NODE_ENV: 'production' }), null);
  });

  it('fake widget hands out the token the API fake verifier accepts', () => {
    assert.equal(FAKE_CAPTCHA_TOKEN, 'fake-captcha-pass');
    const widget = readFileSync(new URL('../captcha/CaptchaField.tsx', import.meta.url), 'utf8');
    assert.match(widget, /FAKE_CAPTCHA_TOKEN/);
  });

  it('keeps no captcha secret or verifier on the web server', () => {
    const port = readFileSync(new URL('../captcha/captcha.ts', import.meta.url), 'utf8');
    assert.equal(/verify|secret|siteverify/i.test(port.replace(/\/\*[\s\S]*?\*\//g, '')), false);
  });
});

describe('parseReportRequest', () => {
  it('accepts an AC-26.1 reason, an optional note and a captcha token', () => {
    assert.deepEqual(parseReportRequest({ reason: 'spam', captcha_token: 'x' }), {
      reason: 'spam',
      note: null,
      captchaToken: 'x',
    });
    assert.deepEqual(parseReportRequest({ reason: 'other', note: '  hi  ', captcha_token: 'x' }), {
      reason: 'other',
      note: 'hi',
      captchaToken: 'x',
    });
  });

  it('refuses unknown reasons, long notes and junk', () => {
    assert.equal(parseReportRequest({ reason: 'nudity', captcha_token: 'x' }), null);
    assert.equal(
      parseReportRequest({ reason: 'spam', note: 'x'.repeat(501), captcha_token: 'x' }),
      null,
    );
    assert.equal(parseReportRequest({ reason: 'spam', note: 5, captcha_token: 'x' }), null);
    assert.equal(parseReportRequest({ reason: 'spam', captcha_token: 'x'.repeat(5000) }), null);
    assert.equal(parseReportRequest([]), null);
    assert.equal(parseReportRequest('spam'), null);
  });

  it('tells a missing captcha apart (400 captcha)', () => {
    assert.equal(parseReportRequest({ reason: 'spam' }), 'no-captcha');
    assert.equal(parseReportRequest({ reason: 'spam', captcha_token: '' }), 'no-captcha');
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
  const REPORT = { reason: 'spam', note: null, captchaToken: FAKE_CAPTCHA_TOKEN } as const;

  it('posts the report with captcha_token, the service credential and the visitor IP', async () => {
    const api = fakeFetch(() => Response.json({ status: 'received' }, { status: 202 }));
    const outcome = await submitReport(
      TOKEN,
      { reason: 'hate', note: 'see card 2', captchaToken: 'widget-token' },
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
    // Exactly the PublicReportRequest fields (the API refuses unknown fields).
    assert.deepEqual(JSON.parse(String(call?.init?.body)), {
      token: TOKEN,
      reason: 'hate',
      captcha_token: 'widget-token',
      note: 'see card 2',
    });
  });

  it('omits an empty note and never calls the API for a malformed token', async () => {
    const api = fakeFetch(() => new Response(null, { status: 202 }));
    assert.equal(await submitReport('bad', REPORT, CONFIG, '1.1.1.1', api.fetch), 'gone');
    assert.equal(api.calls.length, 0);
    await submitReport(TOKEN, REPORT, CONFIG, '1.1.1.1', api.fetch);
    assert.deepEqual(JSON.parse(String(api.calls[0]?.init?.body)), {
      token: TOKEN,
      reason: 'spam',
      captcha_token: FAKE_CAPTCHA_TOKEN,
    });
  });

  it('maps API answers to outcomes (a 400 is the failed captcha)', async () => {
    const cases: [number, string][] = [
      [202, 'accepted'],
      [400, 'captcha'],
      [404, 'gone'],
      [429, 'limited'],
      [401, 'unavailable'],
      [503, 'unavailable'],
      [200, 'unavailable'],
    ];
    for (const [status, outcome] of cases) {
      const api = fakeFetch(() => new Response(null, { status }));
      assert.equal(await submitReport(TOKEN, REPORT, CONFIG, '1.1.1.1', api.fetch), outcome);
    }
    const down = fakeFetch(() => Promise.reject(new Error('down')));
    assert.equal(await submitReport(TOKEN, REPORT, CONFIG, '1.1.1.1', down.fetch), 'unavailable');
  });
});

describe('report route source', () => {
  const route = readFileSync(
    new URL('../../app/b/[token]/report/route.ts', import.meta.url),
    'utf8',
  );

  it('forwards the captcha to the API instead of verifying it, and never logs', () => {
    assert.equal(/verif(y|ier)\(/.test(route), false);
    assert.match(route, /submitReport\(token, report, config, ip\)/);
    assert.equal(/console\.(log|info|warn|error|debug)/.test(route), false);
  });
});
