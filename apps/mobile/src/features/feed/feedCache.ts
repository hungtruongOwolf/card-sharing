import { secureKeyValueStore, utf8Length, type KeyValueStore } from '../drafts/draftStore';
import type { CardItem } from '../sharing/cardsApi';

/**
 * Offline copy of the feed (T-80, AC-15.1): the newest items' metadata only, so the feed still shows who sent what
 * when the phone is offline. Signed media URLs are never written here: they are short-lived capabilities (SR-8) and
 * a stored URL would outlive its TTL; offline items show a placeholder instead of the picture.
 *
 * Storage: the drafts' SecureStore seam (`WHEN_UNLOCKED_THIS_DEVICE_ONLY`, no backups), split into chunks under the
 * 2 KB value limit. The copy is replaced on every successful load (so unfriended, blocked or deleted cards drop out
 * on the next sync, SR-9) and cleared on sign-out.
 */
export interface CachedFeedItem {
  readonly id: string;
  readonly senderName: string | null;
  readonly caption: string | null;
  readonly altText: string | null;
  /** Epoch milliseconds. */
  readonly sentAt: number;
  readonly myReaction: string | null;
}

export interface CachedFeed {
  /** Epoch milliseconds of the load the copy was taken from. */
  readonly savedAt: number;
  readonly items: readonly CachedFeedItem[];
}

export interface FeedCache {
  /** The saved copy, or null when there is none, it is too old or unreadable. */
  load(now?: number): Promise<CachedFeed | null>;
  /** Replaces the copy with the newest {@link FEED_CACHE_MAX_ITEMS} items. */
  save(items: readonly CardItem[], now?: number): Promise<void>;
  clear(): Promise<void>;
}

/** prd-mvp AC-15.1: the last 50 items are cached offline. */
export const FEED_CACHE_MAX_ITEMS = 50;
/** An offline copy older than this is not shown (it would mostly show cards that may be gone). */
export const FEED_CACHE_MAX_AGE_MS = 7 * 24 * 3_600_000;
export const FEED_CACHE_META_KEY = 'cs.feed.cache.v1.meta';
export const feedCacheChunkKey = (index: number) => `cs.feed.cache.v1.chunk.${index}`;

/** Stays under expo-secure-store's 2048-byte value limit on Android, with room for the array brackets. */
const MAX_CHUNK_BYTES = 1900;
const MAX_CHUNKS = 40;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_TEXT = 400;

const toCached = (card: CardItem): CachedFeedItem => ({
  id: card.id,
  senderName: card.sender.displayName,
  caption: card.caption,
  altText: card.altText,
  sentAt: card.sentAt,
  myReaction: card.myReaction,
});

const text = (value: unknown): string | null =>
  typeof value === 'string' && value.length > 0 && value.length <= MAX_TEXT ? value : null;

function parseItem(value: unknown): CachedFeedItem | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Record<string, unknown>;
  if (typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  if (typeof v.sentAt !== 'number' || !Number.isFinite(v.sentAt)) return null;
  return {
    id: v.id,
    senderName: text(v.senderName),
    caption: text(v.caption),
    altText: text(v.altText),
    sentAt: v.sentAt,
    myReaction: text(v.myReaction),
  };
}

/** Splits items into JSON arrays that each fit one secure-store value; an item too big alone is skipped. */
function chunk(items: readonly CachedFeedItem[]): string[] {
  const chunks: string[] = [];
  let current: string[] = [];
  let bytes = 2;
  for (const item of items) {
    const json = JSON.stringify(item);
    const size = utf8Length(json) + 1;
    if (size + 2 > MAX_CHUNK_BYTES) continue;
    if (bytes + size > MAX_CHUNK_BYTES) {
      chunks.push(`[${current.join(',')}]`);
      current = [];
      bytes = 2;
    }
    current.push(json);
    bytes += size;
  }
  if (current.length > 0) chunks.push(`[${current.join(',')}]`);
  return chunks.slice(0, MAX_CHUNKS);
}

export function createFeedCache(kv: KeyValueStore = secureKeyValueStore()): FeedCache {
  // Serialized: a save never interleaves with a clear or another save.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };

  const readMeta = async (): Promise<{ savedAt: number; chunks: number } | null> => {
    try {
      const meta: unknown = JSON.parse((await kv.getItem(FEED_CACHE_META_KEY)) ?? 'null');
      if (typeof meta !== 'object' || meta === null) return null;
      const { savedAt, chunks } = meta as { savedAt?: unknown; chunks?: unknown };
      if (typeof savedAt !== 'number' || !Number.isSafeInteger(chunks)) return null;
      const count = chunks as number;
      return count >= 0 && count <= MAX_CHUNKS ? { savedAt, chunks: count } : null;
    } catch {
      return null;
    }
  };

  const removeAll = async () => {
    const meta = await readMeta();
    await kv.deleteItem(FEED_CACHE_META_KEY);
    for (let i = 0; i < (meta?.chunks ?? 0); i++) await kv.deleteItem(feedCacheChunkKey(i));
  };

  return {
    load: (now = Date.now()) =>
      enqueue(async () => {
        const meta = await readMeta();
        if (!meta || now - meta.savedAt > FEED_CACHE_MAX_AGE_MS || meta.savedAt > now + 60_000) {
          return null;
        }
        const items: CachedFeedItem[] = [];
        for (let i = 0; i < meta.chunks; i++) {
          try {
            const parsed: unknown = JSON.parse((await kv.getItem(feedCacheChunkKey(i))) ?? '[]');
            if (Array.isArray(parsed)) {
              for (const raw of parsed) {
                const item = parseItem(raw);
                if (item) items.push(item);
              }
            }
          } catch {
            // A corrupt chunk loses its items, never the whole feed.
          }
        }
        return { savedAt: meta.savedAt, items: items.slice(0, FEED_CACHE_MAX_ITEMS) };
      }),
    save: (cards, now = Date.now()) =>
      enqueue(async () => {
        const chunks = chunk(cards.slice(0, FEED_CACHE_MAX_ITEMS).map(toCached));
        await removeAll();
        for (const [i, value] of chunks.entries()) await kv.setItem(feedCacheChunkKey(i), value);
        await kv.setItem(
          FEED_CACHE_META_KEY,
          JSON.stringify({ savedAt: now, chunks: chunks.length }),
        );
      }),
    clear: () => enqueue(removeAll),
  };
}

let deviceCache: FeedCache | null = null;

/** The device's feed cache (SecureStore-backed), created on first use. */
export function deviceFeedCache(): FeedCache {
  deviceCache ??= createFeedCache();
  return deviceCache;
}
