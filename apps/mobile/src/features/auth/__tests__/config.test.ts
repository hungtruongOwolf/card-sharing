import { ApiConfigError, parseApiBaseUrl } from '../config';

describe('parseApiBaseUrl', () => {
  it('accepts https and strips trailing slashes', () => {
    expect(parseApiBaseUrl('https://api.example.com/', false)).toBe('https://api.example.com');
    expect(parseApiBaseUrl('https://api.example.com:8443/base/', false)).toBe(
      'https://api.example.com:8443/base',
    );
  });

  it('allows plain http only to local hosts in development builds', () => {
    expect(parseApiBaseUrl('http://192.168.1.20:8080', true)).toBe('http://192.168.1.20:8080');
    expect(parseApiBaseUrl('http://localhost:8080', true)).toBe('http://localhost:8080');
    expect(() => parseApiBaseUrl('http://localhost:8080', false)).toThrow(ApiConfigError);
    expect(() => parseApiBaseUrl('http://api.example.com', true)).toThrow(ApiConfigError);
  });

  it('rejects missing, credentialed or query-carrying values', () => {
    expect(() => parseApiBaseUrl(undefined, true)).toThrow(ApiConfigError);
    expect(() => parseApiBaseUrl('', true)).toThrow(ApiConfigError);
    expect(() => parseApiBaseUrl('https://user:pw@api.example.com', true)).toThrow(ApiConfigError);
    expect(() => parseApiBaseUrl('https://api.example.com/?x=1', true)).toThrow(ApiConfigError);
    expect(() => parseApiBaseUrl('ftp://api.example.com', true)).toThrow(ApiConfigError);
  });
});
