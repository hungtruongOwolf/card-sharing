import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import nextConfig from '../next.config.ts';

type Rule = {
  source: string;
  has?: { type: string; key: string }[];
  missing?: { type: string; key: string }[];
  headers: { key: string; value: string }[];
};

const rules = (await nextConfig.headers?.()) as Rule[];

/** Effective headers for a path, given the request headers present (later rules win, like Next.js). */
function headersFor(source: string, requestHeaders: string[] = []): Map<string, string> {
  const result = new Map<string, string>();
  for (const rule of rules) {
    const matches =
      rule.source === '/:path*' ||
      rule.source === source ||
      (rule.source.endsWith(':token*') && source.startsWith(rule.source.replace(':token*', '')));
    const hasOk = (rule.has ?? []).every((h) => requestHeaders.includes(h.key));
    const missingOk = (rule.missing ?? []).every((h) => !requestHeaders.includes(h.key));
    if (matches && hasOk && missingOk) {
      for (const header of rule.headers) result.set(header.key, header.value);
    }
  }
  return result;
}

describe('public board page headers (T-132, SR-17)', () => {
  const page = headersFor('/b/:token');

  it('sends no referrer and is never indexed', () => {
    assert.equal(page.get('Referrer-Policy'), 'no-referrer');
    assert.equal(page.get('X-Robots-Tag'), 'noindex, nofollow, noarchive');
  });

  it('keeps HSTS, nosniff and framing protection', () => {
    assert.match(page.get('Strict-Transport-Security') ?? '', /max-age=\d{8,}/);
    assert.equal(page.get('X-Content-Type-Options'), 'nosniff');
    assert.equal(page.get('X-Frame-Options'), 'DENY');
  });

  it('lets a shared cache keep the HTML for 60 s at most', () => {
    assert.equal(page.get('Cache-Control'), 'public, max-age=0, s-maxage=60');
  });

  it('never caches RSC payloads of the page', () => {
    for (const header of ['rsc', 'next-router-prefetch', 'next-router-state-tree']) {
      assert.equal(headersFor('/b/:token', [header]).get('Cache-Control'), 'private, no-store');
    }
  });

  it('leaves Cache-Control of the OG route to the route itself', () => {
    const og = headersFor('/b/:token/og');
    assert.equal(og.get('Referrer-Policy'), 'no-referrer');
    assert.equal(og.has('Cache-Control'), false);
  });
});

describe('invite page headers', () => {
  it('stays private and unindexed', () => {
    const invite = headersFor('/i/:token');
    assert.equal(invite.get('Referrer-Policy'), 'no-referrer');
    assert.equal(invite.get('Cache-Control'), 'private, no-store');
  });
});
