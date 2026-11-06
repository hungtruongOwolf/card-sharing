import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { buildCsp } from './csp.ts';

const base = { nonce: 'bm9uY2U=', isDev: false, mediaOrigin: 'https://media.example' };

function directives(csp: string): Map<string, string> {
  return new Map(
    csp.split('; ').map((part) => {
      const [name = '', ...rest] = part.split(' ');
      return [name, rest.join(' ')];
    }),
  );
}

describe('buildCsp', () => {
  it('locks the public board page down', () => {
    const csp = directives(buildCsp({ ...base, pathname: '/b/AbCdEfGhIjKlMnOpQrSt_-' }));
    assert.equal(csp.get('script-src'), "'self' 'nonce-bm9uY2U=' 'strict-dynamic'");
    assert.equal(csp.get('style-src'), "'self' 'nonce-bm9uY2U='");
    assert.equal(csp.get('img-src'), "'self' https://media.example");
    assert.equal(csp.get('frame-ancestors'), "'none'");
    assert.equal(csp.get('object-src'), "'none'");
    assert.equal(csp.get('default-src'), "'self'");
    assert.ok(csp.has('upgrade-insecure-requests'));
  });

  it('never allows inline code, eval or WebAssembly on share pages in production', () => {
    for (const pathname of ['/b/AbCdEfGhIjKlMnOpQrSt_-', '/i/AbCdEfGhIjKlMnOpQrSt_-']) {
      const csp = buildCsp({ ...base, pathname });
      assert.doesNotMatch(csp, /unsafe-inline|unsafe-eval|wasm-unsafe-eval|blob:|data:/);
    }
  });

  it('allows only same-origin images on a board page without a media origin', () => {
    const csp = directives(buildCsp({ ...base, mediaOrigin: null, pathname: '/b/x' }));
    assert.equal(csp.get('img-src'), "'self'");
  });

  it('keeps invite pages to same-origin images', () => {
    const csp = directives(buildCsp({ ...base, pathname: '/i/x' }));
    assert.equal(csp.get('img-src'), "'self'");
    assert.equal(csp.get('frame-ancestors'), "'none'");
  });
});
