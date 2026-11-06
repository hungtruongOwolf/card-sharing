import type { components } from '@card-sharing/api-client';

import type { AuthClient } from '../auth';

type WidgetTokenView = components['schemas']['WidgetTokenView'];

/** A widget token (validated `WidgetTokenView`): read-only, `GET /v1/widget` only, a child of this device's session. */
export interface WidgetToken {
  readonly token: string;
  /** Epoch milliseconds. */
  readonly expiresAt: number;
}

export type WidgetTokenErrorKind =
  /** 403 `FORBIDDEN` / `ONBOARDING_INCOMPLETE` / `ACCOUNT_SUSPENDED`. */
  | 'not_allowed'
  /** 429 `RATE_LIMITED` (20 a day per device). */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class WidgetTokenError extends Error {
  override name = 'WidgetTokenError';
  constructor(
    readonly kind: WidgetTokenErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `widget token: ${kind}` : `widget token: ${kind} (${status})`);
  }
}

/** `POST /v1/me/widget-token`: mints a token and replaces any earlier one of this session (shown once). */
export interface WidgetTokenApi {
  mint(): Promise<WidgetToken>;
}

/** Tokens are opaque; this only bounds what is stored (printable ASCII, no whitespace). */
const TOKEN = /^[\x21-\x7e]{16,512}$/;

export function parseWidgetToken(body: unknown): WidgetToken | null {
  if (typeof body !== 'object' || body === null) return null;
  const v = body as Partial<Record<keyof WidgetTokenView, unknown>>;
  if (typeof v.token !== 'string' || !TOKEN.test(v.token)) return null;
  const expiresAt = typeof v.expires_at === 'string' ? Date.parse(v.expires_at) : NaN;
  if (!Number.isFinite(expiresAt)) return null;
  return { token: v.token, expiresAt };
}

function failure(status: number): WidgetTokenError {
  if (status === 403) return new WidgetTokenError('not_allowed', status);
  if (status === 429) return new WidgetTokenError('rate_limited', status);
  return new WidgetTokenError('unavailable', status);
}

export function createWidgetTokenApi(fetcher: AuthClient['fetch']): WidgetTokenApi {
  return {
    async mint() {
      const response = await fetcher('/v1/me/widget-token', { method: 'POST' });
      if (response.status !== 201) throw failure(response.status);
      const token = parseWidgetToken(await response.json().catch(() => null));
      if (!token) throw new WidgetTokenError('unavailable', response.status);
      return token;
    },
  };
}
