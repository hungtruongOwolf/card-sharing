import type { ParamValue } from '@card-sharing/effects-registry';

import { secureKeyValueStore, utf8Length, type KeyValueStore } from '../drafts/draftStore';
import type { StyleBlock, StyleRef } from '../cards/localCard';
import type { Audience } from '../sharing/cardsApi';
import type { SendJob, SendMedia } from '../sharing/sendCard';

/**
 * The offline outbox (T-78, US-13 AC-13.3, US-17 AC-17.3): sends and reactions that could not reach the server yet.
 * Each entry keeps the exact request it will repeat (a send keeps its job, so the idempotency key and body never
 * change between retries and the server answers a repeat with the first result instead of a second card).
 *
 * Storage: the drafts' SecureStore seam (`WHEN_UNLOCKED_THIS_DEVICE_ONLY`, no backups), one value per entry plus an
 * index, every value under the 2 KB limit. The outbox belongs to the signed-in account and is cleared on sign-out.
 */
export type OutboxState =
  /** Waiting for its next attempt. */
  | 'pending'
  /** Stopped: refused by the server, or still failing after 24 h ("Not sent - retry or delete"). */
  | 'failed';

interface EntryBase {
  readonly id: string;
  /** Epoch milliseconds when it was queued (or retried by hand); the 24 h window counts from here. */
  readonly createdAt: number;
  readonly attempts: number;
  /** Epoch milliseconds of the next automatic attempt. */
  readonly nextAttemptAt: number;
  readonly state: OutboxState;
}

export interface SendEntry extends EntryBase {
  readonly kind: 'send';
  readonly job: SendJob;
}

export interface ReactionEntry extends EntryBase {
  readonly kind: 'reaction';
  readonly cardId: string;
  /** The reaction to set, or null to remove mine. */
  readonly reaction: string | null;
}

export type OutboxEntry = SendEntry | ReactionEntry;

export interface OutboxStore {
  /** Oldest first; unreadable records are skipped. */
  list(): Promise<OutboxEntry[]>;
  /** Creates or replaces. Throws {@link OutboxStoreError} when full or the entry is too large. */
  put(entry: OutboxEntry): Promise<void>;
  remove(id: string): Promise<void>;
  clear(): Promise<void>;
}

export const OUTBOX_INDEX_KEY = 'cs.outbox.index.v1';
export const outboxKey = (id: string) => `cs.outbox.item.v1.${id}`;
export const MAX_OUTBOX_ENTRIES = 50;
const MAX_ENTRY_BYTES = 2048;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const ENTRY_ID = /^(?:[0-9a-f-]{36}|r-[0-9a-f-]{36})$/i;
const REACTION = /^react\.[a-z0-9_-]{1,40}$/;
const MAX_CAPTION = 400;

export const reactionEntryId = (cardId: string) => `r-${cardId}`;

export class OutboxStoreError extends Error {
  override readonly name = 'OutboxStoreError';
  constructor(readonly reason: 'full' | 'tooLarge' | 'invalid') {
    super(`outbox: ${reason}`);
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isUuid = (value: unknown): value is string => typeof value === 'string' && UUID.test(value);
const isTime = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

const isParamValue = (value: unknown): value is ParamValue =>
  typeof value === 'boolean' ||
  typeof value === 'string' ||
  (typeof value === 'number' && Number.isFinite(value));

function parseRef(value: unknown): StyleRef | null {
  if (!isRecord(value) || typeof value.id !== 'string' || !Number.isSafeInteger(value.version)) {
    return null;
  }
  if (!isRecord(value.params) || !Object.values(value.params).every(isParamValue)) return null;
  return {
    id: value.id,
    version: value.version as number,
    params: value.params as Record<string, ParamValue>,
  };
}

function parseStyle(value: unknown): StyleBlock | null {
  if (!isRecord(value)) return null;
  const cut = parseRef(value.cut);
  if (!cut) return null;
  if (value.finish === undefined) return { cut };
  const finish = parseRef(value.finish);
  return finish ? { cut, finish } : null;
}

function parseAudience(value: unknown): Audience | null {
  if (!isRecord(value)) return null;
  if (value.mode === 'all') return { mode: 'all' };
  if (value.mode !== 'subset' || !Array.isArray(value.recipientIds)) return null;
  const ids = value.recipientIds;
  return ids.length > 0 && ids.every(isUuid) ? { mode: 'subset', recipientIds: ids } : null;
}

function parseMedia(value: unknown): SendMedia | null | undefined {
  if (value === null) return null;
  if (!isRecord(value) || !isUuid(value.render)) return undefined;
  const optional = (v: unknown) => (v === null ? null : isUuid(v) ? v : undefined);
  const mask = optional(value.mask);
  const original = optional(value.original);
  if (mask === undefined || original === undefined) return undefined;
  return { render: value.render, mask, original };
}

function parseJob(value: unknown): SendJob | null {
  if (!isRecord(value) || !isUuid(value.clientCardId) || typeof value.cardId !== 'string') {
    return null;
  }
  const audience = parseAudience(value.audience);
  const style = parseStyle(value.style);
  const media = parseMedia(value.media);
  const caption =
    value.caption === null
      ? null
      : typeof value.caption === 'string' && value.caption.length <= MAX_CAPTION
        ? value.caption
        : undefined;
  if (!audience || !style || media === undefined || caption === undefined) return null;
  if (typeof value.keepOriginal !== 'boolean') return null;
  // ADR-0010 reply target: kept only when it is a card id, so a queued reply stays a reply.
  if (value.replyToCardId !== undefined && !isUuid(value.replyToCardId)) return null;
  return {
    clientCardId: value.clientCardId,
    cardId: value.cardId,
    audience,
    keepOriginal: value.keepOriginal,
    style,
    caption,
    media,
    ...(isUuid(value.replyToCardId) ? { replyToCardId: value.replyToCardId } : {}),
  };
}

/** Reads a stored entry defensively (storage is outside the type system). */
export function parseEntry(raw: string | null, expectedId?: string): OutboxEntry | null {
  if (!raw) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isRecord(value) || typeof value.id !== 'string' || !ENTRY_ID.test(value.id)) return null;
  if (expectedId !== undefined && value.id !== expectedId) return null;
  if (!isTime(value.createdAt) || !isTime(value.nextAttemptAt)) return null;
  if (!Number.isSafeInteger(value.attempts) || (value.attempts as number) < 0) return null;
  if (value.state !== 'pending' && value.state !== 'failed') return null;
  const base = {
    id: value.id,
    createdAt: value.createdAt,
    attempts: value.attempts as number,
    nextAttemptAt: value.nextAttemptAt,
    state: value.state,
  } as const;
  if (value.kind === 'send') {
    const job = parseJob(value.job);
    return job && job.clientCardId === value.id ? { ...base, kind: 'send', job } : null;
  }
  if (value.kind === 'reaction') {
    if (!isUuid(value.cardId) || value.id !== reactionEntryId(value.cardId)) return null;
    const reaction =
      value.reaction === null
        ? null
        : typeof value.reaction === 'string' && REACTION.test(value.reaction)
          ? value.reaction
          : undefined;
    if (reaction === undefined) return null;
    return { ...base, kind: 'reaction', cardId: value.cardId, reaction };
  }
  return null;
}

function parseIndex(raw: string | null): string[] {
  try {
    const parsed: unknown = JSON.parse(raw ?? '[]');
    if (!Array.isArray(parsed)) return [];
    return [
      ...new Set(parsed.filter((id): id is string => typeof id === 'string' && ENTRY_ID.test(id))),
    ];
  } catch {
    return [];
  }
}

export function createOutboxStore(kv: KeyValueStore = secureKeyValueStore()): OutboxStore {
  // Serialized like the drafts store: an index update never races another one.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };
  const readIndex = async () => parseIndex(await kv.getItem(OUTBOX_INDEX_KEY));

  return {
    list: () =>
      enqueue(async () => {
        const entries = await Promise.all(
          (await readIndex()).map(async (id) => parseEntry(await kv.getItem(outboxKey(id)), id)),
        );
        return entries
          .filter((entry): entry is OutboxEntry => entry !== null)
          .sort((a, b) => a.createdAt - b.createdAt);
      }),
    put: (entry) =>
      enqueue(async () => {
        if (!ENTRY_ID.test(entry.id)) throw new OutboxStoreError('invalid');
        const value = JSON.stringify(entry);
        if (utf8Length(value) > MAX_ENTRY_BYTES) throw new OutboxStoreError('tooLarge');
        const index = await readIndex();
        const known = index.includes(entry.id);
        if (!known && index.length >= MAX_OUTBOX_ENTRIES) throw new OutboxStoreError('full');
        await kv.setItem(outboxKey(entry.id), value);
        if (!known) await kv.setItem(OUTBOX_INDEX_KEY, JSON.stringify([...index, entry.id]));
      }),
    remove: (id) =>
      enqueue(async () => {
        if (!ENTRY_ID.test(id)) return;
        const index = await readIndex();
        await kv.deleteItem(outboxKey(id));
        if (index.includes(id)) {
          await kv.setItem(OUTBOX_INDEX_KEY, JSON.stringify(index.filter((known) => known !== id)));
        }
      }),
    clear: () =>
      enqueue(async () => {
        for (const id of await readIndex()) await kv.deleteItem(outboxKey(id));
        await kv.deleteItem(OUTBOX_INDEX_KEY);
      }),
  };
}
