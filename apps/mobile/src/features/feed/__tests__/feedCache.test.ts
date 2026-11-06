import { memoryKeyValueStore, utf8Length } from '../../drafts/draftStore';
import type { CardItem } from '../../sharing/cardsApi';
import {
  createFeedCache,
  FEED_CACHE_MAX_AGE_MS,
  FEED_CACHE_MAX_ITEMS,
  FEED_CACHE_META_KEY,
} from '../feedCache';

const NOW = Date.parse('2026-11-05T10:00:00Z');

const item = (n: number, caption: string | null = `card ${n}`): CardItem => ({
  id: `c0000000-0000-4000-8000-${String(n).padStart(12, '0')}`,
  sender: { id: '00000000-0000-4000-8000-000000000001', displayName: 'Mai' },
  caption,
  altText: null,
  media: {
    renderUrl: `https://media.test/${n}?exp=1&sig=secret`,
    maskUrl: null,
    expiresAt: NOW + 600_000,
  },
  sentAt: NOW - n * 60_000,
  isMine: false,
  status: null,
  hold: null,
  myReaction: n === 1 ? 'react.heart' : null,
});

describe('feed cache', () => {
  it('keeps the newest 50 items without any media URL, in secure-store sized chunks', async () => {
    const kv = memoryKeyValueStore();
    const cache = createFeedCache(kv);
    const long = 'x'.repeat(300);
    await cache.save(
      Array.from({ length: 60 }, (_, i) => item(i + 1, long)),
      NOW,
    );
    for (const value of kv.items.values()) {
      expect(value).not.toContain('https://');
      expect(value).not.toContain('sig=');
      expect(utf8Length(value)).toBeLessThanOrEqual(2048);
    }
    const loaded = await cache.load(NOW);
    expect(loaded?.items).toHaveLength(FEED_CACHE_MAX_ITEMS);
    expect(loaded?.items[0]).toEqual({
      id: item(1).id,
      senderName: 'Mai',
      caption: long,
      altText: null,
      sentAt: item(1).sentAt,
      myReaction: 'react.heart',
    });
  });

  it('replaces the copy on every save, so removed cards drop out', async () => {
    const kv = memoryKeyValueStore();
    const cache = createFeedCache(kv);
    await cache.save([item(1), item(2), item(3)], NOW);
    await cache.save([item(2)], NOW);
    expect((await cache.load(NOW))?.items.map((i) => i.id)).toEqual([item(2).id]);
    expect([...kv.items.keys()].filter((k) => k.includes('chunk'))).toHaveLength(1);
  });

  it('ignores old or corrupt copies and clears everything', async () => {
    const kv = memoryKeyValueStore();
    const cache = createFeedCache(kv);
    await cache.save([item(1)], NOW);
    expect(await cache.load(NOW + FEED_CACHE_MAX_AGE_MS + 1)).toBeNull();
    kv.items.set('cs.feed.cache.v1.chunk.0', '[{"id":"../x","sentAt":1}, 7]');
    expect((await cache.load(NOW))?.items).toEqual([]);
    kv.items.set(FEED_CACHE_META_KEY, 'not json');
    expect(await cache.load(NOW)).toBeNull();
    await cache.save([item(1)], NOW);
    await cache.clear();
    expect(kv.items.size).toBe(0);
    expect(await cache.load(NOW)).toBeNull();
  });
});
