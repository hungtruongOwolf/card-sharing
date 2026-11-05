import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';

type Schemas = components['schemas'];

/** Another account as request, friend and block lists show it (validated `UserSummaryView`). */
export interface Person {
  readonly id: string;
  readonly displayName: string;
  /** Lowercase handle without `@`; null when the account has none or the server sent a malformed one. */
  readonly handle: string | null;
}

export type RequestDirection = 'incoming' | 'outgoing';

/** One open friend request (validated `RequestItemView`). */
export interface FriendRequest {
  readonly id: string;
  /** The sender (incoming) or the recipient (outgoing). */
  readonly person: Person;
  /** Epoch milliseconds. */
  readonly createdAt: number;
  /** Epoch milliseconds; open requests expire after 14 days (AC-5.4). */
  readonly expiresAt: number;
  /** AC-5.5: only ever set on a minor's incoming requests; the server decides, the app only shows it. */
  readonly adultNoMutualFriends: boolean;
}

export interface BlockedPerson {
  readonly person: Person;
  /** Epoch milliseconds. */
  readonly blockedAt: number;
}

/** My invite link (validated `InviteResponse`). */
export interface MyInvite {
  /** `<share host>/i/<token>`. */
  readonly url: string;
  /** Epoch milliseconds; set only for a minor's invite (AC-4.4), null = valid until reset. */
  readonly expiresAt: number | null;
}

/** What the app may show before adding an inviter (validated `InvitePreviewResponse`). */
export interface InvitePreview {
  readonly displayName: string;
  /** The caller opened their own invite ("That's you!"). */
  readonly isSelf: boolean;
}

/** Privacy settings in force (validated `SettingsView`). */
export interface PrivacySettings {
  readonly findableByHandle: boolean;
  /** False under the region's discovery age: the switch cannot be turned on (the server says so, 403). */
  readonly canBeFindable: boolean;
}

/** The one account an exact-handle search found (validated `SearchResultView`); no id, handle or age. */
export interface SearchResult {
  /** Opaque, valid 10 minutes for this account only; sent back as `search_ref`. */
  readonly ref: string;
  readonly displayName: string;
}

/** Who a friend request goes to: the owner of an opened invite, or a handle-search result. */
export type RequestTarget = { readonly inviteToken: string } | { readonly searchRef: string };

export type FriendsErrorKind =
  /** 404 `NOT_FOUND`: unknown, gone or not ours (the server answers the same for all of them). */
  | 'not_found'
  /** 404 `INVITE_EXPIRED`: the invite was reset, expired or used up. */
  | 'invite_expired'
  /** 409 `ALREADY_FRIENDS`. */
  | 'already_friends'
  /** 409 `FRIEND_LIMIT_REACHED`: either side has 20 friends (AC-5.2). */
  | 'friend_limit'
  /** 403 `POLICY_DISALLOWED`: not allowed for this account (e.g. findable under the discovery age). */
  | 'not_allowed'
  /** 400 `VALIDATION_FAILED` (e.g. sending a request to one's own invite). */
  | 'invalid'
  /** 429 `RATE_LIMITED`: limits and caps (20 pending, 30 a day, searches); try later. */
  | 'rate_limited'
  /** Network, 5xx, `IDEMPOTENCY_IN_PROGRESS`, unexpected status or body; retry later. */
  | 'unavailable';

export class FriendsError extends Error {
  override name = 'FriendsError';
  constructor(
    readonly kind: FriendsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `friends: ${kind}` : `friends: ${kind} (${status})`);
  }
}

/**
 * The social endpoints (T-37..T-47, api-mvp "Social"). Every answer is validated here; the screens only see
 * these types. Authorization, caps, blocks and age rules are the server's: nothing here decides them.
 */
export interface FriendsApi {
  /** `GET /v1/me/invite`. */
  myInvite(): Promise<MyInvite>;
  /** `POST /v1/me/invite/reset`: the old link stops working (AC-4.3). */
  resetInvite(): Promise<MyInvite>;
  /** `GET /v1/invites/{token}`. */
  previewInvite(token: string): Promise<InvitePreview>;
  /**
   * `POST /v1/friend-requests` with `Idempotency-Key`. Resolves the same for a delivered request and one the
   * server silently dropped (a block): the app cannot tell, by design.
   */
  sendRequest(target: RequestTarget, idempotencyKey: string): Promise<void>;
  /** `GET /v1/friend-requests?direction=…`, newest first. */
  listRequests(direction: RequestDirection): Promise<FriendRequest[]>;
  /** `POST /v1/friend-requests/{id}/accept`: resolves with the new friend. */
  accept(requestId: string): Promise<Person>;
  /** `POST /v1/friend-requests/{id}/decline` (silent to the sender). */
  decline(requestId: string): Promise<void>;
  /** `DELETE /v1/friend-requests/{id}` (my outgoing request). */
  cancel(requestId: string): Promise<void>;
  /** `GET /v1/friends`. */
  listFriends(): Promise<Person[]>;
  /** `DELETE /v1/friends/{userId}` (silent, AC-6.2). */
  removeFriend(userId: string): Promise<void>;
  /** `POST /v1/blocks` (silent, AC-26.2). */
  block(userId: string): Promise<void>;
  /** `GET /v1/blocks`. */
  listBlocks(): Promise<BlockedPerson[]>;
  /** `DELETE /v1/blocks/{userId}` (does not restore the friendship). */
  unblock(userId: string): Promise<void>;
  /** `settings` of `GET /v1/me`. */
  privacySettings(): Promise<PrivacySettings>;
  /** `PATCH /v1/me/settings`: resolves with the settings now in force. */
  setFindable(findable: boolean): Promise<PrivacySettings>;
  /** `POST /v1/users/search`: null = "No one found", the same for every hidden account. */
  searchHandle(handle: string): Promise<SearchResult | null>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
/** 128 random bits, base64url without padding (api-mvp T-37). */
const INVITE_TOKEN = /^[A-Za-z0-9_-]{22}$/;
const INVITE_URL = /^https?:\/\/[^\s/?#]+(?:\/[^\s?#]*)?\/i\/([A-Za-z0-9_-]{22})$/;
const HANDLE = /^[a-z0-9._]{3,20}$/;
/** Search refs are sealed base64url; bound the size so a hostile body cannot grow a request. */
const SEARCH_REF = /^[A-Za-z0-9_-]{1,512}$/;
/** The server stores at most 30 graphemes; anything far beyond that is not a display name. */
const MAX_NAME_LENGTH = 200;

export const isInviteToken = (value: string): boolean => INVITE_TOKEN.test(value);
export const isUserId = (value: string): boolean => UUID.test(value);

/** The token of an invite link the server issued, or null when `url` is not one. */
export function inviteTokenFromUrl(url: string): string | null {
  return INVITE_URL.exec(url)?.[1] ?? null;
}

const record = (value: unknown): Record<string, unknown> | null =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const timestamp = (value: unknown): number | null => {
  const ms = typeof value === 'string' ? Date.parse(value) : NaN;
  return Number.isFinite(ms) ? ms : null;
};

function displayNameFrom(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const name = value.trim();
  return name.length > 0 && name.length <= MAX_NAME_LENGTH ? name : null;
}

/** Validates a `UserSummaryView`; a malformed one is dropped, never guessed. */
export function personFrom(value: unknown): Person | null {
  const v = record(value);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  const displayName = displayNameFrom(v.display_name);
  if (!displayName) return null;
  const handle = typeof v.handle === 'string' && HANDLE.test(v.handle) ? v.handle : null;
  return { id: v.id, displayName, handle };
}

function requestFrom(value: unknown): FriendRequest | null {
  const v = record(value);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  const person = personFrom(v.user);
  const createdAt = timestamp(v.created_at);
  const expiresAt = timestamp(v.expires_at);
  if (!person || createdAt === null || expiresAt === null) return null;
  return {
    id: v.id,
    person,
    createdAt,
    expiresAt,
    adultNoMutualFriends: v.hint === 'adult_no_mutual_friends',
  };
}

function blockedFrom(value: unknown): BlockedPerson | null {
  const v = record(value);
  const person = personFrom(v?.user);
  const blockedAt = timestamp(v?.blocked_at);
  return person && blockedAt !== null ? { person, blockedAt } : null;
}

/** `{items: [...]}` with each item validated by `item`; malformed items are dropped. */
function listFrom<T>(body: unknown, item: (value: unknown) => T | null): T[] | null {
  const items = record(body)?.items;
  if (!Array.isArray(items)) return null;
  return items.map(item).filter((x): x is T => x !== null);
}

export const parseRequestList = (body: unknown) => listFrom(body, requestFrom);
export const parseFriendList = (body: unknown) => listFrom(body, personFrom);
export const parseBlockList = (body: unknown) => listFrom(body, blockedFrom);

export function parseInvite(body: unknown): MyInvite | null {
  const v = record(body);
  if (!v || typeof v.url !== 'string' || inviteTokenFromUrl(v.url) === null) return null;
  if (v.expires_at === null || v.expires_at === undefined) return { url: v.url, expiresAt: null };
  const expiresAt = timestamp(v.expires_at);
  return expiresAt === null ? null : { url: v.url, expiresAt };
}

export function parseInvitePreview(body: unknown): InvitePreview | null {
  const v = record(body);
  const displayName = displayNameFrom(record(v?.inviter)?.display_name);
  if (!v || !displayName || typeof v.is_self !== 'boolean') return null;
  return { displayName, isSelf: v.is_self };
}

export function parseSettings(body: unknown): PrivacySettings | null {
  const v = record(body);
  if (!v || typeof v.findable_by_handle !== 'boolean' || typeof v.can_be_findable !== 'boolean') {
    return null;
  }
  // Never show "findable" on an account that cannot be: the server says it is always false then.
  return {
    findableByHandle: v.findable_by_handle && v.can_be_findable,
    canBeFindable: v.can_be_findable,
  };
}

/** `undefined` = not a search response; `null` = "No one found". */
export function parseSearch(body: unknown): SearchResult | null | undefined {
  const v = record(body);
  if (!v || !('result' in v)) return undefined;
  if (v.result === null) return null;
  const result = record(v.result);
  const displayName = displayNameFrom(result?.display_name);
  if (!result || typeof result.ref !== 'string' || !SEARCH_REF.test(result.ref) || !displayName) {
    return undefined;
  }
  return { ref: result.ref, displayName };
}

async function failure(response: Response): Promise<FriendsError> {
  const { status } = response;
  const code = await problemCode(response);
  switch (code) {
    case 'INVITE_EXPIRED':
      return new FriendsError('invite_expired', status);
    case 'ALREADY_FRIENDS':
      return new FriendsError('already_friends', status);
    case 'FRIEND_LIMIT_REACHED':
      return new FriendsError('friend_limit', status);
    case 'POLICY_DISALLOWED':
      return new FriendsError('not_allowed', status);
  }
  if (status === 404) return new FriendsError('not_found', status);
  if (status === 429) return new FriendsError('rate_limited', status);
  if (status === 400) return new FriendsError('invalid', status);
  return new FriendsError('unavailable', status);
}

/**
 * Social calls over the authorized `fetch` of the auth client (T-21). A revoked or expired session surfaces
 * as `AuthError` from the fetcher and signs the device out; everything else is a {@link FriendsError}.
 * Ids and tokens reach a URL path only after validation; anything else is the server's own 404.
 *
 * No `X-Attestation` header yet: the app has no App Attest / Play Integrity module, and the server runs
 * attestation in `log_only` mode during the beta (api-mvp "Attestation").
 */
export function createFriendsApi(fetcher: AuthClient['fetch']): FriendsApi {
  const send = async (
    path: string,
    method: string,
    body?: unknown,
    headers?: Record<string, string>,
  ) => {
    const response = await fetcher(path, {
      method,
      headers: {
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
        ...headers,
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    if (!response.ok) throw await failure(response);
    return response;
  };

  /** Sends, then validates the JSON body with `parse`; a body that is not the expected shape is an error. */
  const read = async <T>(
    parse: (body: unknown) => T | null | undefined,
    path: string,
    method: string,
    body?: unknown,
  ): Promise<T> => {
    const response = await send(path, method, body);
    const parsed = parse(await response.json().catch(() => undefined));
    if (parsed === null || parsed === undefined) {
      throw new FriendsError('unavailable', response.status);
    }
    return parsed;
  };

  const checkedId = (id: string): string => {
    if (!UUID.test(id)) throw new FriendsError('not_found');
    return encodeURIComponent(id);
  };

  const meSettings = (body: unknown) => parseSettings(record(body)?.settings);

  return {
    myInvite: () => read(parseInvite, '/v1/me/invite', 'GET'),
    resetInvite: () => read(parseInvite, '/v1/me/invite/reset', 'POST'),
    async previewInvite(token) {
      if (!INVITE_TOKEN.test(token)) throw new FriendsError('not_found');
      return read(parseInvitePreview, `/v1/invites/${token}`, 'GET');
    },
    async sendRequest(target, idempotencyKey) {
      let body: Schemas['CreateFriendRequest'];
      if ('inviteToken' in target) {
        if (!INVITE_TOKEN.test(target.inviteToken)) throw new FriendsError('not_found');
        body = { invite_token: target.inviteToken };
      } else {
        if (!SEARCH_REF.test(target.searchRef)) throw new FriendsError('not_found');
        body = { search_ref: target.searchRef };
      }
      await send('/v1/friend-requests', 'POST', body, { 'Idempotency-Key': idempotencyKey });
    },
    listRequests: (direction) =>
      read(parseRequestList, `/v1/friend-requests?direction=${direction}`, 'GET'),
    async accept(requestId) {
      const path = `/v1/friend-requests/${checkedId(requestId)}/accept`;
      return read((body) => personFrom(record(body)?.friend), path, 'POST');
    },
    async decline(requestId) {
      await send(`/v1/friend-requests/${checkedId(requestId)}/decline`, 'POST');
    },
    async cancel(requestId) {
      await send(`/v1/friend-requests/${checkedId(requestId)}`, 'DELETE');
    },
    listFriends: () => read(parseFriendList, '/v1/friends', 'GET'),
    async removeFriend(userId) {
      await send(`/v1/friends/${checkedId(userId)}`, 'DELETE');
    },
    async block(userId) {
      checkedId(userId);
      const body: Schemas['BlockRequest'] = { user_id: userId };
      await send('/v1/blocks', 'POST', body);
    },
    listBlocks: () => read(parseBlockList, '/v1/blocks', 'GET'),
    async unblock(userId) {
      await send(`/v1/blocks/${checkedId(userId)}`, 'DELETE');
    },
    privacySettings: () => read(meSettings, '/v1/me', 'GET'),
    setFindable(findable) {
      const body: Schemas['SettingsRequest'] = { findable_by_handle: findable };
      return read(parseSettings, '/v1/me/settings', 'PATCH', body);
    },
    async searchHandle(handle) {
      const body: Schemas['SearchRequest'] = { handle };
      const response = await send('/v1/users/search', 'POST', body);
      const result = parseSearch(await response.json().catch(() => undefined));
      if (result === undefined) throw new FriendsError('unavailable', response.status);
      return result;
    },
  };
}
