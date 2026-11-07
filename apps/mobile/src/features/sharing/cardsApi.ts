import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import { AuthError, type AuthClient } from '../auth';
import type { StyleBlock } from '../cards/localCard';

type Schemas = components['schemas'];

/** Upload kinds the app sends (`avatar` belongs to the profile flow). */
export type MediaKind = 'card_render' | 'mask' | 'original';
export type MediaContentType = 'image/png' | 'image/jpeg';

/** One of my uploads (validated `MediaView`); never carries a URL. */
export interface UploadedMedia {
  readonly id: string;
  readonly kind: MediaKind;
  readonly state: string;
}

/** Who receives a card (AC-13.1): every current friend, or a subset of them. */
export type Audience =
  { readonly mode: 'all' } | { readonly mode: 'subset'; readonly recipientIds: readonly string[] };

/** Everything `POST /v1/cards` needs; the server decides owner, status and the final recipient set. */
export interface NewCard {
  /** UUID; also the `Idempotency-Key`, so every retry of one send is the same request. */
  readonly clientCardId: string;
  readonly renderMediaId: string;
  readonly maskMediaId: string | null;
  readonly originalMediaId: string | null;
  readonly style: StyleBlock;
  readonly caption: string | null;
  readonly audience: Audience;
  /**
   * ADR-0010: reply with this card to a friend's card. The server sends it to that card's sender only (the audience
   * must then be exactly that sender) and answers 404 when I may no longer see the original.
   */
  readonly replyToCardId?: string;
}

/** Owner-only card status (`CardView.status`). */
export type CardStatus = 'pending_scan' | 'held_review' | 'active' | 'blocked' | 'failed';

/** Validated `SentCardView` (the 202 of `POST /v1/cards`). */
export interface SentCard {
  readonly id: string;
  readonly status: CardStatus;
  /** How many current friends will receive it (who was dropped is never revealed). */
  readonly recipientCount: number;
}

/** Classifier hold on my own card (validated `CardHoldView`). */
export interface CardHold {
  /** `none`, `pending`, `released`, `upheld` or `expired`. */
  readonly appeal: string;
  /** Epoch milliseconds. */
  readonly expiresAt: number;
}

/** Per-viewer signed media of a card; short-lived capabilities, never persisted past `expiresAt`. */
export interface CardMedia {
  readonly renderUrl: string;
  readonly maskUrl: string | null;
  /** Epoch milliseconds; refetch the card (not the URL) after this. */
  readonly expiresAt: number;
}

/** The finish a card was made with (`style.finish`); `params` are untrusted until resolved against the registry. */
export interface CardFinish {
  readonly id: string;
  readonly version: number;
  readonly params: Readonly<Record<string, unknown>>;
}

/** A card as the caller may see it (validated `CardView`). */
export interface CardItem {
  readonly id: string;
  readonly sender: { readonly id: string; readonly displayName: string | null };
  readonly caption: string | null;
  readonly altText: string | null;
  /** Null while the card is not delivered (my pending or held card) or when the body was malformed. */
  readonly media: CardMedia | null;
  /** The card's finish (drawn live in the full-screen view when allowed), or null. */
  readonly finish: CardFinish | null;
  /** Epoch milliseconds. */
  readonly sentAt: number;
  readonly isMine: boolean;
  /** Owner only. */
  readonly status: CardStatus | null;
  /** Owner only, when the classifier held the card. */
  readonly hold: CardHold | null;
  /** My own reaction (recipients only). */
  readonly myReaction: string | null;
  /**
   * The card's render was made by an AI effect: show the visible "AI" badge. Read from `ai_generated`, which the
   * backend lane is adding to `CardView` (genai-effects.md "Not yet"); absent = false until it ships.
   */
  readonly aiGenerated?: boolean;
  /**
   * ADR-0010: this card is a reply to the card with this id (`reply_to`, resolved per viewer: absent when I may no
   * longer see the original). The feed and viewer say "replied to your card".
   */
  readonly replyTo?: { readonly cardId: string };
}

/** One page of my feed (validated `FeedPage`), newest delivery first. */
export interface FeedPage {
  readonly items: readonly CardItem[];
  /** Opaque cursor of the next page; null on the last page. */
  readonly nextCursor: string | null;
}

export type CardsErrorKind =
  /** 404 `NOT_FOUND`: missing, not mine, or not visible to me (the server answers the same for all). */
  | 'not_found'
  /** 409 `APPEAL_ALREADY_FILED`: this hold was already appealed or decided (T-148). */
  | 'already_appealed'
  /** 400 `VALIDATION_FAILED` (audience, caption, mask size, bad image header). */
  | 'invalid'
  /** 413 `MEDIA_TOO_LARGE`. */
  | 'too_large'
  /** 415 `MEDIA_TYPE_UNSUPPORTED`. */
  | 'unsupported'
  /** 422 `UNKNOWN_REGISTRY_ID`: a style id the server no longer allows. */
  | 'unknown_style'
  /** 422 `IDEMPOTENCY_KEY_REUSED`: an earlier attempt with this card id had a different body. */
  | 'key_reused'
  /** 403 (`ONBOARDING_INCOMPLETE`, `ACCOUNT_SUSPENDED`, `ATTESTATION_FAILED`). */
  | 'not_allowed'
  /** 429 `RATE_LIMITED` (100 sends a day, upload and read budgets). */
  | 'rate_limited'
  /** Network, 5xx, 409 `IDEMPOTENCY_IN_PROGRESS`, unexpected status or body: retry later. */
  | 'unavailable';

export class CardsError extends Error {
  override name = 'CardsError';
  constructor(
    readonly kind: CardsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `cards: ${kind}` : `cards: ${kind} (${status})`);
  }
}

/** Whether retrying the same request later can succeed (offline, server busy, rate limit). */
export function isRetryable(error: unknown): boolean {
  if (error instanceof AuthError) return error.kind === 'unavailable';
  return (
    error instanceof CardsError && (error.kind === 'unavailable' || error.kind === 'rate_limited')
  );
}

/**
 * The card and media endpoints (T-70..T-84, api-mvp "Cards, feed, widget, reactions"). Every answer is validated
 * here; screens only see these types. Visibility, recipients, rate limits and content gates are the server's.
 */
export interface CardsApi {
  /** `POST /v1/media?kind=` with the raw image bytes; the same bytes of the same kind return the same upload. */
  uploadMedia(
    kind: MediaKind,
    contentType: MediaContentType,
    bytes: Uint8Array,
  ): Promise<UploadedMedia>;
  /** `POST /v1/cards` with `Idempotency-Key` = `clientCardId` (202). */
  createCard(card: NewCard): Promise<SentCard>;
  /** `GET /v1/cards/{id}`. */
  getCard(id: string): Promise<CardItem>;
  /** `GET /v1/feed?cursor=` (30 per page). A tampered or expired cursor is `invalid` (400). */
  feed(cursor?: string | null): Promise<FeedPage>;
  /**
   * `POST /v1/cards/{id}/appeal` (T-148, AC-28.3): asks a moderator to review the classifier hold on my card, once
   * per hold. 409 `APPEAL_ALREADY_FILED` → `already_appealed`; not my held card (or the hold expired) → `not_found`.
   */
  appealHold(cardId: string): Promise<void>;
  /** `PUT /v1/cards/{id}/reaction {kind}`: sets or replaces my one reaction (AC-17.2). */
  setReaction(cardId: string, kind: string): Promise<void>;
  /** `DELETE /v1/cards/{id}/reaction`. */
  clearReaction(cardId: string): Promise<void>;
  /**
   * `POST /v1/feed/purges {card_ids}` (T-83, SR-9): which of these cached cards must be deleted now (unfriend,
   * block, deleted card or sender; an unknown id is answered like a purged one). Any number of ids: sent in
   * batches of {@link PURGE_CHECK_MAX}. Only ids that were asked about are returned.
   */
  checkPurges(cardIds: readonly string[]): Promise<string[]>;
}

/** Most ids one `POST /v1/feed/purges` accepts. */
export const PURGE_CHECK_MAX = 100;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const STATUSES: readonly CardStatus[] = [
  'pending_scan',
  'held_review',
  'active',
  'blocked',
  'failed',
];
const MEDIA_KINDS: readonly MediaKind[] = ['card_render', 'mask', 'original'];
/** Bounds for server text so a hostile body cannot grow the UI. */
const MAX_NAME_LENGTH = 200;
const MAX_TEXT_LENGTH = 1000;
const MAX_URL_LENGTH = 4096;
const MAX_REACTION_LENGTH = 64;
/** Feed cursors are base64url (position + HMAC tag); bound the size so a hostile body cannot grow a request. */
const CURSOR = /^[A-Za-z0-9_-]{1,512}$/;
/** Registry ids (`fx.holo`): bounded so a hostile body cannot grow the UI. */
const STYLE_ID = /^[a-z0-9][a-z0-9._-]{0,63}$/;
/**
 * A server `Date` header closer than this to the device clock is ignored: it has one-second precision, and the
 * screens refetch {@link CardMedia.expiresAt} 30 s early anyway.
 */
const MIN_CLOCK_SKEW_MS = 2_000;
/** Reaction ids as config/client.yaml names them (`react.heart`). */
const REACTION = /^react\.[a-z0-9_-]{1,40}$/;

export const isReactionKind = (value: string): boolean => REACTION.test(value);

export const isCardId = (value: string): boolean => UUID.test(value);

const record = (value: unknown): Record<string, unknown> | null =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const timestamp = (value: unknown): number | null => {
  const ms = typeof value === 'string' ? Date.parse(value) : NaN;
  return Number.isFinite(ms) ? ms : null;
};

const optionalText = (value: unknown, max = MAX_TEXT_LENGTH): string | null =>
  typeof value === 'string' && value.trim().length > 0 && value.length <= max ? value : null;

const status = (value: unknown): CardStatus | null =>
  STATUSES.find((known) => known === value) ?? null;

/** Only http(s) URLs are media; anything else (javascript:, file:, data:) is dropped. */
export function mediaUrl(value: unknown): string | null {
  if (typeof value !== 'string' || value.length > MAX_URL_LENGTH) return null;
  return /^https?:\/\/[^\s/?#]+[^\s]*$/i.test(value) ? value : null;
}

export function parseMedia(body: unknown): UploadedMedia | null {
  const v = record(record(body)?.media);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id) || typeof v.state !== 'string') {
    return null;
  }
  const kind = MEDIA_KINDS.find((known) => known === v.kind);
  return kind ? { id: v.id, kind, state: v.state } : null;
}

export function parseSentCard(body: unknown): SentCard | null {
  const v = record(record(body)?.card);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  const cardStatus = status(v.status);
  const count = v.recipient_count;
  if (!cardStatus || !Number.isSafeInteger(count) || (count as number) < 0) return null;
  return { id: v.id, status: cardStatus, recipientCount: count as number };
}

function holdFrom(value: unknown): CardHold | null {
  const v = record(value);
  const expiresAt = timestamp(v?.expires_at);
  if (!v || typeof v.appeal !== 'string' || expiresAt === null) return null;
  return { appeal: v.appeal, expiresAt };
}

function mediaFrom(value: unknown): CardMedia | null {
  const v = record(value);
  const renderUrl = mediaUrl(v?.render_url);
  const expiresAt = timestamp(v?.expires_at);
  if (!v || !renderUrl || expiresAt === null) return null;
  return { renderUrl, maskUrl: mediaUrl(v.mask_url), expiresAt };
}

function finishFrom(style: unknown): CardFinish | null {
  const v = record(record(style)?.finish);
  if (!v || typeof v.id !== 'string' || !STYLE_ID.test(v.id)) return null;
  if (!Number.isSafeInteger(v.version) || (v.version as number) < 1) return null;
  return { id: v.id, version: v.version as number, params: record(v.params) ?? {} };
}

/** Validates a `CardView`; a malformed card is dropped, never guessed. `original_url` is not kept. */
export function parseCard(value: unknown): CardItem | null {
  const v = record(value);
  const sender = record(v?.sender);
  const sentAt = timestamp(v?.sent_at);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id) || typeof v.is_mine !== 'boolean') {
    return null;
  }
  if (!sender || typeof sender.id !== 'string' || !UUID.test(sender.id) || sentAt === null) {
    return null;
  }
  const displayName = optionalText(sender.display_name, MAX_NAME_LENGTH);
  return {
    id: v.id,
    sender: { id: sender.id, displayName: displayName?.trim() ?? null },
    caption: optionalText(v.caption),
    altText: optionalText(v.alt_text),
    media: mediaFrom(v.media),
    finish: finishFrom(v.style),
    sentAt,
    isMine: v.is_mine,
    status: v.is_mine ? status(v.status) : null,
    hold: v.is_mine ? holdFrom(v.hold) : null,
    myReaction: optionalText(v.my_reaction, MAX_REACTION_LENGTH),
    // Only when set, so cards without the flag keep their exact shape.
    ...(v.ai_generated === true ? { aiGenerated: true } : {}),
    ...replyToFrom(v.reply_to),
  };
}

function replyToFrom(value: unknown): { replyTo: { cardId: string } } | Record<string, never> {
  const cardId = record(value)?.card_id;
  return typeof cardId === 'string' && UUID.test(cardId) ? { replyTo: { cardId } } : {};
}

/** Validates `PurgeInstructions`; a malformed body is null (nothing is deleted on a guess). */
export function parsePurges(body: unknown): string[] | null {
  const purge = record(body)?.purge;
  if (!Array.isArray(purge)) return null;
  return purge.filter((id): id is string => typeof id === 'string' && UUID.test(id));
}

/** Validates a `FeedPage`; malformed items are dropped, a malformed cursor ends the feed. */
export function parseFeedPage(body: unknown): FeedPage | null {
  const v = record(body);
  if (!v || !Array.isArray(v.items)) return null;
  const items = v.items.map(parseCard).filter((card): card is CardItem => card !== null);
  const nextCursor =
    typeof v.next_cursor === 'string' && CURSOR.test(v.next_cursor) ? v.next_cursor : null;
  return { items, nextCursor };
}

function audienceBody(audience: Audience): Schemas['AudienceRequest'] {
  if (audience.mode === 'all') return { mode: 'all' };
  const ids = [...new Set(audience.recipientIds)];
  if (ids.length === 0 || !ids.every((id) => UUID.test(id))) throw new CardsError('invalid');
  return { mode: 'subset', recipient_ids: ids };
}

/** The exact JSON body of `POST /v1/cards`; deterministic so a retry hashes the same on the server. */
export function createCardBody(card: NewCard): Schemas['CreateCardRequest'] {
  const ids = [
    card.clientCardId,
    card.renderMediaId,
    card.maskMediaId,
    card.originalMediaId,
    card.replyToCardId ?? null,
  ];
  if (!ids.every((id) => id === null || UUID.test(id))) throw new CardsError('invalid');
  return {
    client_card_id: card.clientCardId,
    render_media_id: card.renderMediaId,
    mask_media_id: card.maskMediaId,
    original_media_id: card.originalMediaId,
    style: card.style as unknown as Record<string, never>,
    caption: card.caption,
    audience: audienceBody(card.audience),
    // Only on a reply, so every other body keeps its exact bytes (idempotent retries).
    ...(card.replyToCardId ? { reply_to_card_id: card.replyToCardId } : {}),
  };
}

/**
 * Device clock minus server clock, from the response `Date` header (0 when absent, unreadable or within
 * {@link MIN_CLOCK_SKEW_MS}). Signed-URL expiries are server times; screens compare them with the device clock.
 */
export function clockSkew(response: Response, deviceNow: number): number {
  const serverNow = Date.parse(response.headers?.get('Date') ?? '');
  if (!Number.isFinite(serverNow)) return 0;
  const skew = deviceNow - serverNow;
  return Math.abs(skew) < MIN_CLOCK_SKEW_MS ? 0 : skew;
}

/** The card with its signed-URL expiry moved onto the device clock (#263). */
export function onDeviceClock(card: CardItem, skew: number): CardItem {
  if (skew === 0 || !card.media) return card;
  return { ...card, media: { ...card.media, expiresAt: card.media.expiresAt + skew } };
}

async function failure(response: Response): Promise<CardsError> {
  const { status: code } = response;
  switch (await problemCode(response)) {
    case 'UNKNOWN_REGISTRY_ID':
      return new CardsError('unknown_style', code);
    case 'IDEMPOTENCY_KEY_REUSED':
      return new CardsError('key_reused', code);
    case 'APPEAL_ALREADY_FILED':
      return new CardsError('already_appealed', code);
  }
  if (code === 404) return new CardsError('not_found', code);
  if (code === 400) return new CardsError('invalid', code);
  if (code === 403) return new CardsError('not_allowed', code);
  if (code === 413) return new CardsError('too_large', code);
  if (code === 415) return new CardsError('unsupported', code);
  if (code === 429) return new CardsError('rate_limited', code);
  return new CardsError('unavailable', code);
}

/**
 * Card calls over the authorized `fetch` of the auth client (T-21). A revoked or expired session surfaces as
 * `AuthError` from the fetcher and signs the device out; everything else is a {@link CardsError}. Ids reach a URL
 * path only after validation. No `X-Attestation` header yet: the server runs attestation in `log_only` mode during
 * the beta (api-mvp "Attestation").
 *
 * Signed-URL expiries in returned cards are on the device clock (`now`): a phone whose clock runs far ahead of
 * the server would otherwise treat fresh URLs as expired, and one far behind would keep dead ones (#263).
 */
export function createCardsApi(
  fetcher: AuthClient['fetch'],
  { now = Date.now }: { now?: () => number } = {},
): CardsApi {
  const read = async <T>(
    parse: (body: unknown, skew: number) => T | null,
    path: string,
    init: RequestInit,
  ): Promise<T> => {
    const response = await fetcher(path, init);
    if (!response.ok) throw await failure(response);
    const skew = clockSkew(response, now());
    const parsed = parse(await response.json().catch(() => undefined), skew);
    if (parsed === null) throw new CardsError('unavailable', response.status);
    return parsed;
  };

  const checkedId = (id: string): string => {
    if (!UUID.test(id)) throw new CardsError('not_found');
    return encodeURIComponent(id);
  };

  return {
    uploadMedia(kind, contentType, bytes) {
      return read(parseMedia, `/v1/media?kind=${kind}`, {
        method: 'POST',
        headers: { 'Content-Type': contentType },
        // React Native's fetch sends an ArrayBufferView as raw bytes (Libraries/Network/convertRequestBody);
        // the DOM typings do not list it.
        body: bytes as unknown as BodyInit,
      });
    },
    async createCard(card) {
      const body = createCardBody(card);
      return read(parseSentCard, '/v1/cards', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': card.clientCardId },
        body: JSON.stringify(body),
      });
    },
    async feed(cursor) {
      if (cursor !== undefined && cursor !== null && !CURSOR.test(cursor)) {
        throw new CardsError('invalid');
      }
      const query = cursor ? `?cursor=${encodeURIComponent(cursor)}` : '';
      return read(
        (body, skew) => {
          const page = parseFeedPage(body);
          return page && { ...page, items: page.items.map((card) => onDeviceClock(card, skew)) };
        },
        `/v1/feed${query}`,
        { method: 'GET' },
      );
    },
    async appealHold(cardId) {
      const body: Schemas['AppealRequest'] = {};
      const response = await fetcher(`/v1/cards/${checkedId(cardId)}/appeal`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw await failure(response);
    },
    async setReaction(cardId, kind) {
      if (!REACTION.test(kind)) throw new CardsError('invalid');
      const body: Schemas['ReactionRequest'] = { kind };
      const response = await fetcher(`/v1/cards/${checkedId(cardId)}/reaction`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw await failure(response);
    },
    async clearReaction(cardId) {
      const response = await fetcher(`/v1/cards/${checkedId(cardId)}/reaction`, {
        method: 'DELETE',
      });
      if (!response.ok) throw await failure(response);
    },
    async checkPurges(cardIds) {
      const asked = [...new Set(cardIds)].filter((id) => UUID.test(id));
      const purged: string[] = [];
      for (let i = 0; i < asked.length; i += PURGE_CHECK_MAX) {
        const batch = asked.slice(i, i + PURGE_CHECK_MAX);
        const body: Schemas['PurgeCheckRequest'] = { card_ids: batch };
        const purge = await read(parsePurges, '/v1/feed/purges', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        const inBatch = new Set(batch);
        purged.push(...purge.filter((id) => inBatch.has(id)));
      }
      return purged;
    },
    async getCard(id) {
      return read(
        (body, skew) => {
          const card = parseCard(record(body)?.card);
          return card && onDeviceClock(card, skew);
        },
        `/v1/cards/${checkedId(id)}`,
        { method: 'GET' },
      );
    },
  };
}
