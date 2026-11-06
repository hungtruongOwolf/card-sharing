import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  UNKNOWN_VISITOR_IP,
  httpUrl,
  readServiceApiConfig,
  serviceGet,
  visitorIp,
} from './service-api.ts';

const CREDENTIAL = 'c'.repeat(32);

describe('readServiceApiConfig', () => {
  it('needs an API URL and a long credential', () => {
    assert.equal(readServiceApiConfig({}), null);
    assert.equal(
      readServiceApiConfig({ API_URL: 'http://api', API_SERVICE_CREDENTIAL: 'short' }),
      null,
    );
    assert.equal(
      readServiceApiConfig({ API_URL: 'not a url', API_SERVICE_CREDENTIAL: CREDENTIAL }),
      null,
    );
    assert.equal(
      readServiceApiConfig({ API_URL: 'file:///etc/passwd', API_SERVICE_CREDENTIAL: CREDENTIAL }),
      null,
    );
    const config = readServiceApiConfig({
      API_URL: 'https://api.example',
      API_SERVICE_CREDENTIAL: CREDENTIAL,
      CLIENT_IP_HEADER: 'Fly-Client-IP',
    });
    assert.equal(config?.apiUrl.origin, 'https://api.example');
    assert.equal(config?.clientIpHeader, 'fly-client-ip');
  });
});

describe('httpUrl', () => {
  it('accepts absolute http(s) URLs only', () => {
    assert.equal(httpUrl('https://cdn.example/a')?.origin, 'https://cdn.example');
    assert.equal(httpUrl('javascript:alert(1)'), null);
    assert.equal(httpUrl('data:image/png;base64,AAAA'), null);
    assert.equal(httpUrl('/relative'), null);
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

describe('serviceGet', () => {
  it('turns a failed request into null without throwing', async () => {
    const failing = (async () => {
      throw new Error('connect failed');
    }) as typeof fetch;
    const config = {
      apiUrl: new URL('http://api.internal'),
      credential: CREDENTIAL,
      clientIpHeader: null,
    };
    assert.equal(await serviceGet('/v1/public/x', config, '1.1.1.1', failing), null);
  });
});
