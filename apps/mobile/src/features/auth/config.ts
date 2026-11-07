/**
 * API base URL from `EXPO_PUBLIC_API_URL`. Everything with the `EXPO_PUBLIC_` prefix is inlined into the
 * JS bundle and readable by anyone, so it may hold only public values (never secrets).
 *
 * Expo inlines the variable only for the literal `process.env.EXPO_PUBLIC_API_URL` expression, so it is read
 * here and nowhere else. Release builds require https; development builds may use plain http to a loopback or
 * private-network host (a laptop running the API, reached from a phone on the same Wi-Fi).
 */
export class ApiConfigError extends Error {
  override name = 'ApiConfigError';
}

// React Native's `URL` polyfill does not implement every getter on all versions, so parse by hand.
const BASE_URL = /^(https?):\/\/(\[[0-9a-f:]+\]|[a-z0-9.-]+)(:\d{1,5})?(\/[A-Za-z0-9._~/-]*)?$/i;
const PRIVATE_HOST =
  /^(localhost|\[::1\]|127\.\d+\.\d+\.\d+|10\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+|172\.(1[6-9]|2\d|3[01])\.\d+\.\d+)$/i;

/** Loopback or private-network host (RFC 1918, IPv6 loopback): where plain http is tolerated in development. */
export function isPrivateHost(host: string): boolean {
  return PRIVATE_HOST.test(host);
}

export function parseApiBaseUrl(raw: string | undefined, allowLocalHttp: boolean): string {
  const value = raw?.trim();
  if (!value) throw new ApiConfigError('EXPO_PUBLIC_API_URL is not set');
  const match = BASE_URL.exec(value);
  if (!match)
    throw new ApiConfigError('EXPO_PUBLIC_API_URL must be a plain http(s) origin or path');
  const scheme = (match[1] ?? '').toLowerCase();
  const host = match[2] ?? '';
  const localHttp = allowLocalHttp && scheme === 'http' && isPrivateHost(host);
  if (scheme !== 'https' && !localHttp) {
    throw new ApiConfigError('EXPO_PUBLIC_API_URL must use https');
  }
  return value.replace(/\/+$/, '');
}

/** Reads and validates the base URL lazily, so a missing value fails the first request, not app start. */
export function apiBaseUrl(): string {
  return parseApiBaseUrl(process.env.EXPO_PUBLIC_API_URL, __DEV__);
}
