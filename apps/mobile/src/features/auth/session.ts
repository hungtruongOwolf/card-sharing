import type { components } from '@card-sharing/api-client';

/** Token pair as returned by `POST /v1/auth/refresh` (and, later, the sign-in routes). */
export type TokenPair = components['schemas']['TokenPair'];

/**
 * The device session held by the app (ADR-0004): an opaque 10-minute access token and a single-use
 * refresh token. Lives only in memory and in the secure store, never in AsyncStorage, logs or the query cache.
 */
export interface Session {
  readonly accessToken: string;
  readonly refreshToken: string;
  /** Epoch milliseconds. */
  readonly accessExpiresAt: number;
}

const MAX_TOKEN_LENGTH = 256;

const isToken = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= MAX_TOKEN_LENGTH;

/**
 * Validates a token pair from the network. The response is untrusted input like any other: a
 * malformed pair is rejected instead of being stored.
 */
export function sessionFromTokenPair(body: unknown): Session | null {
  if (typeof body !== 'object' || body === null) return null;
  const pair = body as Partial<Record<keyof TokenPair, unknown>>;
  if (pair.token_type !== 'Bearer' || !isToken(pair.access_token) || !isToken(pair.refresh_token)) {
    return null;
  }
  const expiresAt =
    typeof pair.access_expires_at === 'string' ? Date.parse(pair.access_expires_at) : NaN;
  if (!Number.isFinite(expiresAt)) return null;
  return {
    accessToken: pair.access_token,
    refreshToken: pair.refresh_token,
    accessExpiresAt: expiresAt,
  };
}

/** Parses a session read back from the secure store; anything unexpected counts as no session. */
export function parseStoredSession(raw: string | null): Session | null {
  if (raw === null) return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null) return null;
    const s = value as Partial<Record<keyof Session, unknown>>;
    if (!isToken(s.accessToken) || !isToken(s.refreshToken)) return null;
    if (typeof s.accessExpiresAt !== 'number' || !Number.isFinite(s.accessExpiresAt)) return null;
    return {
      accessToken: s.accessToken,
      refreshToken: s.refreshToken,
      accessExpiresAt: s.accessExpiresAt,
    };
  } catch {
    return null;
  }
}

export const serializeSession = (session: Session): string =>
  JSON.stringify({
    accessToken: session.accessToken,
    refreshToken: session.refreshToken,
    accessExpiresAt: session.accessExpiresAt,
  });
