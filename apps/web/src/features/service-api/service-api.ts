import { isIP } from 'node:net';

/**
 * Server-side access to the API's `svc` routes (`/v1/public/*`): the web server calls them with the
 * service credential and forwards the visitor IP. Server-only: the credential comes from server env
 * (no NEXT_PUBLIC_ prefix) and this module is imported only by server components, route handlers and
 * the proxy, so it never reaches a client bundle. Nothing here logs a URL, a token or the credential.
 */

/** Minimum credential length, as enforced by the API (`API_SERVICE_CREDENTIAL`). */
const MIN_CREDENTIAL = 32;

/** Bucket used when no trusted client-IP header is configured: every visitor shares one limit (fails safe). */
export const UNKNOWN_VISITOR_IP = '0.0.0.0';

/** How long a page waits for the API before it renders its "unavailable" state. */
export const SERVICE_TIMEOUT_MS = 3000;

export type ServiceApiConfig = {
  readonly apiUrl: URL;
  readonly credential: string;
  /** Header the edge proxy overwrites with the visitor IP (e.g. `fly-client-ip`), if any. */
  readonly clientIpHeader: string | null;
};

export type Env = Readonly<Record<string, string | undefined>>;

/**
 * Server env: `API_URL` (base URL of the API), `API_SERVICE_CREDENTIAL` (secret, same value as the
 * API's) and optional `CLIENT_IP_HEADER`. `null` when the calls cannot be configured; pages then
 * render their "unavailable" state.
 */
export function readServiceApiConfig(env: Env): ServiceApiConfig | null {
  const credential = env.API_SERVICE_CREDENTIAL?.trim() ?? '';
  const rawUrl = env.API_URL?.trim() ?? '';
  if (credential.length < MIN_CREDENTIAL || rawUrl === '') {
    return null;
  }
  const apiUrl = httpUrl(rawUrl);
  if (apiUrl === null) {
    return null;
  }
  const header = env.CLIENT_IP_HEADER?.trim().toLowerCase() ?? '';
  return { apiUrl, credential, clientIpHeader: header === '' ? null : header };
}

/** Parses an absolute http(s) URL; anything else is `null`. */
export function httpUrl(value: string): URL | null {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  return url.protocol === 'https:' || url.protocol === 'http:' ? url : null;
}

/**
 * The visitor IP sent as `X-Client-IP`: only from the configured header that the edge proxy
 * overwrites. Without one (or with a malformed value) every visitor shares one bucket, so a client
 * can never pick its own rate-limit key (e.g. by forging `X-Forwarded-For`).
 */
export function visitorIp(headers: Headers, clientIpHeader: string | null): string {
  if (clientIpHeader === null) {
    return UNKNOWN_VISITOR_IP;
  }
  const value = headers.get(clientIpHeader)?.trim() ?? '';
  return isIP(value) === 0 ? UNKNOWN_VISITOR_IP : value;
}

/**
 * GETs one `svc` route. `null` when the request itself failed (network, timeout, redirect); the
 * caller maps statuses. Redirects are refused and nothing is cached by Next.js.
 */
export async function serviceGet(
  path: string,
  config: ServiceApiConfig,
  clientIp: string,
  fetchImpl: typeof fetch = fetch,
): Promise<Response | null> {
  try {
    return await fetchImpl(new URL(path, config.apiUrl), {
      method: 'GET',
      headers: {
        accept: 'application/json',
        'x-service-credential': config.credential,
        'x-client-ip': clientIp,
      },
      cache: 'no-store',
      redirect: 'error',
      signal: AbortSignal.timeout(SERVICE_TIMEOUT_MS),
    });
  } catch {
    // No logging: the error can carry the request URL, which holds a token.
    return null;
  }
}

/** POSTs a JSON body to one `svc` route. Same rules as [`serviceGet`]. */
export async function servicePost(
  path: string,
  body: unknown,
  config: ServiceApiConfig,
  clientIp: string,
  fetchImpl: typeof fetch = fetch,
): Promise<Response | null> {
  try {
    return await fetchImpl(new URL(path, config.apiUrl), {
      method: 'POST',
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
        'x-service-credential': config.credential,
        'x-client-ip': clientIp,
      },
      body: JSON.stringify(body),
      cache: 'no-store',
      redirect: 'error',
      signal: AbortSignal.timeout(SERVICE_TIMEOUT_MS),
    });
  } catch {
    return null;
  }
}

/** Reads a JSON body; `undefined` when it is not JSON. */
export async function readJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return undefined;
  }
}
