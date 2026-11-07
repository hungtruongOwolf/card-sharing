import type { components } from '@card-sharing/api-client';

import type { AuthClient } from '../auth';

type Schemas = components['schemas'];

/** One switch per alert type (validated `NotificationTypes`). */
export interface NotificationTypes {
  /** New cards from friends (single and batched). */
  readonly cardNew: boolean;
  /** Reactions on my cards. */
  readonly reaction: boolean;
  /** Friend requests and accepted requests. */
  readonly friend: boolean;
  /** My card held for review. */
  readonly cardReview: boolean;
}

export const NOTIFICATION_TYPES: readonly (keyof NotificationTypes)[] = [
  'cardNew',
  'reaction',
  'friend',
  'cardReview',
];

/** Quiet hours in the device's local time (validated `QuietHours`); `end` may be earlier than `start`. */
export interface QuietHours {
  readonly enabled: boolean;
  /** `HH:MM`, 24 h. */
  readonly start: string;
  /** `HH:MM`, 24 h. */
  readonly end: string;
}

/** My notification preferences (validated `NotificationPreferences`). */
export interface NotificationPrefs {
  readonly types: NotificationTypes;
  readonly quietHours: QuietHours;
  /** IANA zone the server applies quiet hours in; null until the app has sent one. */
  readonly timeZone: string | null;
}

export type NotificationsErrorKind =
  /** 400 `VALIDATION_FAILED`: bad time, unknown time zone, empty quiet window. */
  | 'invalid'
  /** 429 `RATE_LIMITED` (30 an hour). */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class NotificationsError extends Error {
  override name = 'NotificationsError';
  constructor(
    readonly kind: NotificationsErrorKind,
    readonly status?: number,
    /** From `Retry-After` on a 429: how long to leave the endpoint alone. */
    readonly retryAfterMs?: number,
  ) {
    super(status === undefined ? `notifications: ${kind}` : `notifications: ${kind} (${status})`);
  }
}

/** `GET/PUT /v1/me/notification-preferences` (T-87, US-31 AC-31.1); always the caller's own. */
export interface NotificationsApi {
  get(): Promise<NotificationPrefs>;
  /** Replaces every field (a null `timeZone` clears it) and answers the stored value. */
  put(prefs: NotificationPrefs): Promise<NotificationPrefs>;
}

/** Strict `HH:MM`, as the server validates it. */
export const CLOCK_TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
/** An IANA name such as `Europe/Paris`, `America/Argentina/Buenos_Aires`, `Etc/GMT+7` or `UTC`. */
const TIME_ZONE = /^[A-Za-z][A-Za-z0-9_+-]*(\/[A-Za-z0-9_+-]+){0,2}$/;
const MAX_TIME_ZONE_LENGTH = 64;

export const isTimeZone = (value: unknown): value is string =>
  typeof value === 'string' && value.length <= MAX_TIME_ZONE_LENGTH && TIME_ZONE.test(value);

function typesFrom(value: unknown): NotificationTypes | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Partial<Record<keyof Schemas['NotificationTypes'], unknown>>;
  if (
    typeof v.card_new !== 'boolean' ||
    typeof v.reaction !== 'boolean' ||
    typeof v.friend !== 'boolean' ||
    typeof v.card_review !== 'boolean'
  ) {
    return null;
  }
  return { cardNew: v.card_new, reaction: v.reaction, friend: v.friend, cardReview: v.card_review };
}

function quietFrom(value: unknown): QuietHours | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Partial<Record<keyof Schemas['QuietHours'], unknown>>;
  if (typeof v.enabled !== 'boolean') return null;
  if (typeof v.start !== 'string' || !CLOCK_TIME.test(v.start)) return null;
  if (typeof v.end !== 'string' || !CLOCK_TIME.test(v.end)) return null;
  return { enabled: v.enabled, start: v.start, end: v.end };
}

/** Parses a `NotificationPreferences` body; anything that is not one is an error, never guessed. */
export function parseNotificationPrefs(body: unknown): NotificationPrefs | null {
  if (typeof body !== 'object' || body === null) return null;
  const v = body as Partial<Record<keyof Schemas['NotificationPreferences'], unknown>>;
  const types = typesFrom(v.types);
  const quietHours = quietFrom(v.quiet_hours);
  if (!types || !quietHours) return null;
  return { types, quietHours, timeZone: isTimeZone(v.time_zone) ? v.time_zone : null };
}

/** The `PUT` body; exactly the contract's fields (the server refuses unknown ones). */
export function notificationPrefsBody(
  prefs: NotificationPrefs,
): Schemas['NotificationPreferences'] {
  return {
    types: {
      card_new: prefs.types.cardNew,
      reaction: prefs.types.reaction,
      friend: prefs.types.friend,
      card_review: prefs.types.cardReview,
    },
    quiet_hours: {
      enabled: prefs.quietHours.enabled,
      start: prefs.quietHours.start,
      end: prefs.quietHours.end,
    },
    time_zone: prefs.timeZone,
  };
}

/** `Retry-After` in seconds (the API never sends the HTTP-date form), as milliseconds. */
function retryAfterMs(response: Response): number | undefined {
  const seconds = Number(response.headers.get('Retry-After'));
  return Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : undefined;
}

function failure(response: Response): NotificationsError {
  const { status } = response;
  if (status === 400) return new NotificationsError('invalid', status);
  if (status === 429) return new NotificationsError('rate_limited', status, retryAfterMs(response));
  return new NotificationsError('unavailable', status);
}

const PATH = '/v1/me/notification-preferences';

/**
 * Preference calls over the authorized `fetch` of the auth client. A revoked or expired session surfaces as
 * `AuthError` from the fetcher; everything else is a {@link NotificationsError}.
 */
export function createNotificationsApi(fetcher: AuthClient['fetch']): NotificationsApi {
  const read = async (response: Response) => {
    if (!response.ok) throw failure(response);
    const prefs = parseNotificationPrefs(await response.json().catch(() => null));
    if (!prefs) throw new NotificationsError('unavailable', response.status);
    return prefs;
  };
  return {
    get: async () => read(await fetcher(PATH, { method: 'GET' })),
    put: async (prefs) =>
      read(
        await fetcher(PATH, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(notificationPrefsBody(prefs)),
        }),
      ),
  };
}
