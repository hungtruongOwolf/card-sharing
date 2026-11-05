import type { components } from '@card-sharing/api-client';

import type { AuthClient } from '../auth';

type SessionView = components['schemas']['SessionView'];

/** One signed-in device as the settings screen shows it (validated `SessionView`, no token material). */
export interface DeviceSession {
  readonly id: string;
  /** Platform and app version, e.g. `ios 1.2.0`; null when the device sent none. */
  readonly clientLabel: string | null;
  /** ISO 3166-1 alpha-2 country seen at sign-in, when known. */
  readonly country: string | null;
  /** Epoch milliseconds. */
  readonly lastSeenAt: number;
  readonly isCurrent: boolean;
}

export type SessionsErrorKind =
  /** 404 `NOT_FOUND`: the session is already gone (revoked elsewhere, expired) or was never ours. */
  | 'not_found'
  /** 429 `RATE_LIMITED`. */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class SessionsError extends Error {
  override name = 'SessionsError';
  constructor(
    readonly kind: SessionsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `sessions: ${kind}` : `sessions: ${kind} (${status})`);
  }
}

/**
 * Session management (T-24, AC-1.4). "Log out everywhere" is not here: it ends this device's session too,
 * so it lives on the auth client (`signOutEverywhere`), which owns the secure store.
 */
export interface SessionsApi {
  /** `GET /v1/sessions`: live device sessions, most recently used first. */
  list(): Promise<DeviceSession[]>;
  /** `DELETE /v1/sessions/{id}`: revokes one session (and its widget token). */
  revoke(id: string): Promise<void>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const COUNTRY = /^[A-Z]{2}$/;
const MAX_LABEL_LENGTH = 64;

/** Validates one `SessionView` from an untrusted body; a malformed row is dropped, never guessed. */
function deviceFrom(value: unknown): DeviceSession | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Partial<Record<keyof SessionView, unknown>>;
  if (typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  if (typeof v.is_current !== 'boolean') return null;
  const lastSeenAt = typeof v.last_seen_at === 'string' ? Date.parse(v.last_seen_at) : NaN;
  if (!Number.isFinite(lastSeenAt)) return null;
  const label =
    typeof v.client_label === 'string' ? v.client_label.trim().slice(0, MAX_LABEL_LENGTH) : '';
  return {
    id: v.id,
    clientLabel: label.length > 0 ? label : null,
    country: typeof v.ip_country === 'string' && COUNTRY.test(v.ip_country) ? v.ip_country : null,
    lastSeenAt,
    isCurrent: v.is_current,
  };
}

/** Parses a `SessionList` body; anything that is not one is an error. */
export function parseSessionList(body: unknown): DeviceSession[] | null {
  if (typeof body !== 'object' || body === null || !('sessions' in body)) return null;
  const { sessions } = body as { sessions: unknown };
  if (!Array.isArray(sessions)) return null;
  return sessions.map(deviceFrom).filter((s): s is DeviceSession => s !== null);
}

function failure(status: number): SessionsError {
  if (status === 404) return new SessionsError('not_found', status);
  if (status === 429) return new SessionsError('rate_limited', status);
  return new SessionsError('unavailable', status);
}

/**
 * Session calls over the authorized `fetch` of the auth client (T-21). A revoked or expired session surfaces
 * as `AuthError` from the fetcher and signs the device out; everything else is a {@link SessionsError}.
 */
export function createSessionsApi(fetcher: AuthClient['fetch']): SessionsApi {
  return {
    async list() {
      const response = await fetcher('/v1/sessions', { method: 'GET' });
      if (!response.ok) throw failure(response.status);
      const sessions = parseSessionList(await response.json().catch(() => null));
      if (!sessions) throw new SessionsError('unavailable', response.status);
      return sessions;
    },
    async revoke(id) {
      // Only ids the list returned are sent; anything else is the same "already gone" as the server's 404.
      if (!UUID.test(id)) throw new SessionsError('not_found');
      const response = await fetcher(`/v1/sessions/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      });
      if (!response.ok) throw failure(response.status);
    },
  };
}
