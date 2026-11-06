import { type ServiceApiConfig, readJson, serviceGet } from '../service-api/service-api.ts';

/**
 * Invite landing (T-48, US-4 AC-4.2/4.4, SR-17): server-side lookup of an invite token through the
 * API's `svc` route (see `features/service-api`). Imported only by the `/i/[token]` server
 * component. Nothing here logs the token.
 */

/** Invite tokens are 128 random bits, base64url without padding (22 characters). */
const INVITE_TOKEN = /^[A-Za-z0-9_-]{22}$/;

/** Bound on a display name from the API (30 grapheme clusters; the API stores at most 480 code points). */
const MAX_DISPLAY_NAME = 480;

export type InviteLookup =
  | { readonly kind: 'found'; readonly displayName: string }
  /** Unknown, malformed, reset, expired, used up, inviter gone: one neutral answer. */
  | { readonly kind: 'gone' }
  /** The API could not answer (not configured, down, rate limited). Says nothing about the token. */
  | { readonly kind: 'unavailable' };

export function isInviteToken(value: string): boolean {
  return INVITE_TOKEN.test(value);
}

/** Looks the token up. Never throws; a bad token is answered without calling the API. */
export async function lookupInvite(
  token: string,
  config: ServiceApiConfig,
  clientIp: string,
  fetchImpl: typeof fetch = fetch,
): Promise<InviteLookup> {
  if (!isInviteToken(token)) {
    return { kind: 'gone' };
  }
  const response = await serviceGet(`/v1/public/invites/${token}`, config, clientIp, fetchImpl);
  if (response === null) {
    return { kind: 'unavailable' };
  }
  if (response.status === 404) {
    return { kind: 'gone' };
  }
  if (!response.ok) {
    return { kind: 'unavailable' };
  }
  const displayName = displayNameOf(await readJson(response));
  return displayName === null ? { kind: 'unavailable' } : { kind: 'found', displayName };
}

function displayNameOf(body: unknown): string | null {
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
