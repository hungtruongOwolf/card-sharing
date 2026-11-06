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

/** A card as the caller may see it (validated `CardView`). */
export interface CardItem {
  readonly id: string;
  readonly sender: { readonly id: string; readonly displayName: string | null };
  readonly caption: string | null;
  readonly altText: string | null;
  /** Null while the card is not delivered (my pending or held card) or when the body was malformed. */
  readonly media: CardMedia | null;
  /** Epoch milliseconds. */
  readonly sentAt: number;
  readonly isMine: boolean;
  /** Owner only. */
  readonly status: CardStatus | null;
  /** Owner only, when the classifier held the card. */
  readonly hold: CardHold | null;
  /** My own reaction (recipients only). */
  readonly myReaction: string | null;
}

export type CardsErrorKind =
  /** 404 `NOT_FOUND`: missing, not mine, or not visible to me (the server answers the same for all). */
  | 'not_found'
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
}

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
    sentAt,
    isMine: v.is_mine,
    status: v.is_mine ? status(v.status) : null,
    hold: v.is_mine ? holdFrom(v.hold) : null,
    myReaction: optionalText(v.my_reaction, MAX_REACTION_LENGTH),
  };
}

function audienceBody(audience: Audience): Schemas['AudienceRequest'] {
  if (audience.mode === 'all') return { mode: 'all' };
  const ids = [...new Set(audience.recipientIds)];
  if (ids.length === 0 || !ids.every((id) => UUID.test(id))) throw new CardsError('invalid');
  return { mode: 'subset', recipient_ids: ids };
}

/** The exact JSON body of `POST /v1/cards`; deterministic so a retry hashes the same on the server. */
export function createCardBody(card: NewCard): Schemas['CreateCardRequest'] {
  const ids = [card.clientCardId, card.renderMediaId, card.maskMediaId, card.originalMediaId];
  if (!ids.every((id) => id === null || UUID.test(id))) throw new CardsError('invalid');
  return {
    client_card_id: card.clientCardId,
    render_media_id: card.renderMediaId,
    mask_media_id: card.maskMediaId,
    original_media_id: card.originalMediaId,
    style: card.style as unknown as Record<string, never>,
    caption: card.caption,
    audience: audienceBody(card.audience),
  };
}

async function failure(response: Response): Promise<CardsError> {
  const { status: code } = response;
  switch (await problemCode(response)) {
    case 'UNKNOWN_REGISTRY_ID':
      return new CardsError('unknown_style', code);
    case 'IDEMPOTENCY_KEY_REUSED':
      return new CardsError('key_reused', code);
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
 */
export function createCardsApi(fetcher: AuthClient['fetch']): CardsApi {
  const read = async <T>(
    parse: (body: unknown) => T | null,
    path: string,
    init: RequestInit,
  ): Promise<T> => {
    const response = await fetcher(path, init);
    if (!response.ok) throw await failure(response);
    const parsed = parse(await response.json().catch(() => undefined));
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
    async getCard(id) {
      return read((body) => parseCard(record(body)?.card), `/v1/cards/${checkedId(id)}`, {
        method: 'GET',
      });
    },
  };
}
