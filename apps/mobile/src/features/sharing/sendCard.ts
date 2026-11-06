import type { Registry } from '@card-sharing/effects-registry';

import { styleBlock, type LocalCard, type StyleBlock } from '../cards/localCard';
import type { PreparedCard } from '../cards/prepare';
import { newIdempotencyKey } from '../friends/idempotencyKey';
import { isCardId, type Audience, type CardsApi, type SentCard } from './cardsApi';

/**
 * One send of one card (T-77, US-13). Everything that goes into `POST /v1/cards` is fixed when the job is made, so
 * every retry is byte-for-byte the same request under the same `Idempotency-Key` (the server replays the first
 * answer instead of creating a second card). The job is plain data so the offline outbox (T-78) can persist it.
 */
export interface SendJob {
  /** The local card's id when it is a UUID (it is for every card the app made), else a fresh one. */
  readonly clientCardId: string;
  readonly cardId: string;
  readonly audience: Audience;
  readonly keepOriginal: boolean;
  readonly style: StyleBlock;
  readonly caption: string | null;
  /** Media ids once every upload of this job succeeded; null until then. */
  readonly media: SendMedia | null;
}

export interface SendMedia {
  readonly render: string;
  readonly mask: string | null;
  readonly original: string | null;
}

export type SendErrorKind =
  /** A style id or version the shipped registry no longer has: edit the card first. */
  'stale_style';

export class SendError extends Error {
  override name = 'SendError';
  constructor(readonly kind: SendErrorKind) {
    super(`send: ${kind}`);
  }
}

/** Builds the job for `card`, or throws {@link SendError} when its style no longer passes the strict check. */
export function newSendJob(
  card: LocalCard,
  registry: Registry,
  audience: Audience,
  keepOriginal: boolean,
): SendJob {
  const style = styleBlock(registry, card.draft);
  if (!style.ok) throw new SendError('stale_style');
  const caption = card.draft.caption.trim();
  return {
    clientCardId: isCardId(card.id) ? card.id : newIdempotencyKey(),
    cardId: card.id,
    audience,
    keepOriginal,
    style: style.style,
    caption: caption.length > 0 ? caption : null,
    media: null,
  };
}

/** The same job under a new card id: only after the server said the old id was used with another body. */
export function withNewCardId(job: SendJob): SendJob {
  return { ...job, clientCardId: newIdempotencyKey() };
}

export interface SendDeps {
  api: CardsApi;
  /** Bakes the card (on tap only); called again on a retry that still needs uploads. */
  prepare: (keepOriginal: boolean) => Promise<PreparedCard>;
  /** Called with the job after its uploads finished, so a caller can keep the media ids for retries. */
  onProgress?: (job: SendJob) => void;
}

/**
 * Runs a job: bakes and uploads (render, then the mask only when the cut style uses it, then the original only
 * with "keep original" on), then creates the card. A partial failure re-bakes and re-uploads on the retry: the bake
 * is byte-stable and the server answers the same bytes with the same upload, so no duplicate media appear.
 */
export async function runSend(
  job: SendJob,
  { api, prepare, onProgress }: SendDeps,
): Promise<{ job: SendJob; sent: SentCard }> {
  let current = job;
  if (!current.media) {
    const prepared = await prepare(current.keepOriginal);
    const render = await api.uploadMedia(
      'card_render',
      prepared.render.contentType,
      prepared.render.bytes,
    );
    const mask = prepared.mask
      ? await api.uploadMedia('mask', prepared.mask.contentType, prepared.mask.bytes)
      : null;
    const original =
      current.keepOriginal && prepared.original
        ? await api.uploadMedia('original', prepared.original.contentType, prepared.original.bytes)
        : null;
    current = {
      ...current,
      media: { render: render.id, mask: mask?.id ?? null, original: original?.id ?? null },
    };
    onProgress?.(current);
  }
  const media = current.media!;
  const sent = await api.createCard({
    clientCardId: current.clientCardId,
    renderMediaId: media.render,
    maskMediaId: media.mask,
    originalMediaId: media.original,
    style: current.style,
    caption: current.caption,
    audience: current.audience,
  });
  return { job: current, sent };
}
