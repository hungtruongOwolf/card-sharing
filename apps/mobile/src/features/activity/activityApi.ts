import type { components } from '@card-sharing/api-client';

import type { AuthClient } from '../auth';

type ActivityItemView = components['schemas']['ActivityItemView'];

/** Kinds the app knows (`ActivityItemView.type`); an item of any other kind is dropped, never guessed. */
export type ActivityType =
  'card.new' | 'reaction.new' | 'friend.request' | 'friend.accepted' | 'card.held';

const TYPES: ReadonlySet<string> = new Set<ActivityType>([
  'card.new',
  'reaction.new',
  'friend.request',
  'friend.accepted',
  'card.held',
]);

/** One activity item (validated `ActivityItemView`): ids only, the app resolves them through their own routes. */
export interface ActivityItem {
  readonly id: string;
  readonly type: ActivityType;
  /** The card (`card.*`, `reaction.new`), friend request (`friend.request`) or user (`friend.accepted`). */
  readonly subjectId: string;
  /** Card sender, reactor or new friend; null when the event names nobody. */
  readonly actorId: string | null;
  /** Epoch milliseconds. */
  readonly createdAt: number;
  readonly unread: boolean;
}

/** One page of my activity (validated `ActivityPage`), newest first. */
export interface ActivityPage {
  readonly items: readonly ActivityItem[];
  /** Unread visible items, counted up to {@link UNREAD_COUNT_CAP} by the server. */
  readonly unreadCount: number;
  readonly nextCursor: string | null;
}

/** The server stops counting unread items here; the badge shows "99+" from this value. */
export const UNREAD_COUNT_CAP = 100;

export type ActivityErrorKind =
  /** 400 `VALIDATION_FAILED`: a cursor the server no longer accepts. */
  | 'invalid'
  /** 404 `NOT_FOUND` on mark read: the item is gone (unfriend, block, delete) or not mine. */
  | 'not_found'
  /** 429 `RATE_LIMITED` (60 a minute). */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class ActivityError extends Error {
  override name = 'ActivityError';
  constructor(
    readonly kind: ActivityErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `activity: ${kind}` : `activity: ${kind} (${status})`);
  }
}

/** `GET /v1/activity` and `POST /v1/activity/read` (T-88, US-31 AC-31.3); always the caller's own. */
export interface ActivityApi {
  page(cursor: string | null): Promise<ActivityPage>;
  /** Marks `upTo` and everything older read; the server never moves the mark backwards. */
  markRead(upTo: string): Promise<void>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_CURSOR_LENGTH = 512;

export const isActivityId = (value: string): boolean => UUID.test(value);

function itemFrom(value: unknown): ActivityItem | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Partial<Record<keyof ActivityItemView, unknown>>;
  if (typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  if (typeof v.type !== 'string' || !TYPES.has(v.type)) return null;
  if (typeof v.subject_id !== 'string' || !UUID.test(v.subject_id)) return null;
  if (typeof v.unread !== 'boolean') return null;
  const createdAt = typeof v.created_at === 'string' ? Date.parse(v.created_at) : NaN;
  if (!Number.isFinite(createdAt)) return null;
  return {
    id: v.id,
    type: v.type as ActivityType,
    subjectId: v.subject_id,
    actorId: typeof v.actor_id === 'string' && UUID.test(v.actor_id) ? v.actor_id : null,
    createdAt,
    unread: v.unread,
  };
}

/** Parses an `ActivityPage` body; a malformed item is dropped, a malformed page is an error. */
export function parseActivityPage(body: unknown): ActivityPage | null {
  if (typeof body !== 'object' || body === null) return null;
  const v = body as { items?: unknown; unread_count?: unknown; next_cursor?: unknown };
  if (!Array.isArray(v.items)) return null;
  if (typeof v.unread_count !== 'number' || !Number.isInteger(v.unread_count)) return null;
  const cursor =
    typeof v.next_cursor === 'string' &&
    v.next_cursor.length > 0 &&
    v.next_cursor.length <= MAX_CURSOR_LENGTH
      ? v.next_cursor
      : null;
  return {
    items: v.items.map(itemFrom).filter((item): item is ActivityItem => item !== null),
    unreadCount: Math.min(Math.max(0, v.unread_count), UNREAD_COUNT_CAP),
    nextCursor: cursor,
  };
}

/** Badge text for an unread count: nothing for 0, "99+" from {@link UNREAD_COUNT_CAP} upwards. */
export function unreadBadge(count: number): string | null {
  if (count <= 0) return null;
  return count >= UNREAD_COUNT_CAP ? '99+' : String(count);
}

function failure(status: number): ActivityError {
  if (status === 400) return new ActivityError('invalid', status);
  if (status === 404) return new ActivityError('not_found', status);
  if (status === 429) return new ActivityError('rate_limited', status);
  return new ActivityError('unavailable', status);
}

/**
 * Activity calls over the authorized `fetch` of the auth client. A revoked or expired session surfaces as
 * `AuthError` from the fetcher; everything else is an {@link ActivityError}.
 */
export function createActivityApi(fetcher: AuthClient['fetch']): ActivityApi {
  return {
    async page(cursor) {
      const query = cursor === null ? '' : `?cursor=${encodeURIComponent(cursor)}`;
      const response = await fetcher(`/v1/activity${query}`, { method: 'GET' });
      if (!response.ok) throw failure(response.status);
      const page = parseActivityPage(await response.json().catch(() => null));
      if (!page) throw new ActivityError('unavailable', response.status);
      return page;
    },
    async markRead(upTo) {
      // Only ids a page returned are sent; anything else is the same "gone" as the server's 404.
      if (!UUID.test(upTo)) throw new ActivityError('not_found');
      const response = await fetcher('/v1/activity/read', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ up_to: upTo }),
      });
      if (!response.ok) throw failure(response.status);
    },
  };
}
