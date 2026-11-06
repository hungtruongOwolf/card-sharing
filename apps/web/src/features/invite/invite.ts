import { isIP } from 'node:net';

/**
 * Invite landing (T-48, US-4 AC-4.2/4.4, SR-17): server-side lookup of an invite token through the
 * API's `svc` route. Server-only: the credential comes from server env (no NEXT_PUBLIC_ prefix) and
 * this module is imported only by the `/i/[token]` server component, so it never reaches a client
 * bundle. Nothing here logs the token or the credential.
 */

/** Invite tokens are 128 random bits, base64url without padding (22 characters). */
const INVITE_TOKEN = /^[A-Za-z0-9_-]{22}$/;

/** Bound on a display name from the API (30 grapheme clusters; the API stores at most 480 code points). */
const MAX_DISPLAY_NAME = 480;

/** Minimum credential length, as enforced by the API (`API_SERVICE_CREDENTIAL`). */
const MIN_CREDENTIAL = 32;

/** Bucket used when no trusted client-IP header is configured: every visitor shares one limit (fails safe). */
export const UNKNOWN_VISITOR_IP = '0.0.0.0';

const LOOKUP_TIMEOUT_MS = 3000;

export type InviteLookup =
  | { readonly kind: 'found'; readonly displayName: string }
  /** Unknown, malformed, reset, expired, used up, inviter gone: one neutral answer. */
  | { readonly kind: 'gone' }
  /** The API could not answer (not configured, down, rate limited). Says nothing about the token. */
  | { readonly kind: 'unavailable' };

export type InviteApiConfig = {
  readonly apiUrl: URL;
  readonly credential: string;
  /** Header the edge proxy overwrites with the visitor IP (e.g. `fly-client-ip`), if any. */
  readonly clientIpHeader: string | null;
};

type Env = Readonly<Record<string, string | undefined>>;

export function isInviteToken(value: string): boolean {
  return INVITE_TOKEN.test(value);
}

/**
 * Server env: `API_URL` (base URL of the API), `API_SERVICE_CREDENTIAL` (secret, same value as the
 * API's) and optional `CLIENT_IP_HEADER`. `null` when the lookup cannot be configured; the page then
 * renders the "unavailable" state.
 */
export function readInviteApiConfig(env: Env): InviteApiConfig | null {
  const credential = env.API_SERVICE_CREDENTIAL?.trim() ?? '';
  const rawUrl = env.API_URL?.trim() ?? '';
  if (credential.length < MIN_CREDENTIAL || rawUrl === '') {
    return null;
  }
  let apiUrl: URL;
  try {
    apiUrl = new URL(rawUrl);
  } catch {
    return null;
  }
  if (apiUrl.protocol !== 'https:' && apiUrl.protocol !== 'http:') {
    return null;
  }
  const header = env.CLIENT_IP_HEADER?.trim().toLowerCase() ?? '';
  return { apiUrl, credential, clientIpHeader: header === '' ? null : header };
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

/** Looks the token up. Never throws; a bad token is answered without calling the API. */
export async function lookupInvite(
  token: string,
  config: InviteApiConfig,
  clientIp: string,
  fetchImpl: typeof fetch = fetch,
): Promise<InviteLookup> {
  if (!isInviteToken(token)) {
    return { kind: 'gone' };
  }
  let response: Response;
  try {
    response = await fetchImpl(new URL(`/v1/public/invites/${token}`, config.apiUrl), {
      method: 'GET',
      headers: {
        accept: 'application/json',
        'x-service-credential': config.credential,
        'x-client-ip': clientIp,
      },
      cache: 'no-store',
      redirect: 'error',
      signal: AbortSignal.timeout(LOOKUP_TIMEOUT_MS),
    });
  } catch {
    // No logging: the error can carry the request URL, which holds the token.
    return { kind: 'unavailable' };
  }
  if (response.status === 404) {
    return { kind: 'gone' };
  }
  if (!response.ok) {
    return { kind: 'unavailable' };
  }
  const displayName = await readDisplayName(response);
  return displayName === null ? { kind: 'unavailable' } : { kind: 'found', displayName };
}

async function readDisplayName(response: Response): Promise<string | null> {
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return null;
  }
  const inviter =
    typeof body === 'object' && body !== null ? (body as { inviter?: unknown }).inviter : null;
  const name =
    typeof inviter === 'object' && inviter !== null
      ? (inviter as { display_name?: unknown }).display_name
      : null;
  if (typeof name !== 'string' || name.trim() === '' || name.length > MAX_DISPLAY_NAME) {
    return null;
  }
  return name;
}
