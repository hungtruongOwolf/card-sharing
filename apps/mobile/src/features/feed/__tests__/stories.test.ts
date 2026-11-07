import { memoryKeyValueStore } from '../../drafts/draftStore';
import type { CardItem } from '../../sharing/cardsApi';
import {
  createSeenStore,
  isUnseen,
  parseSeen,
  SEEN_KEY,
  SEEN_MAX_SENDERS,
  withSeen,
} from '../seenCards';
import { storyRings, storySequence } from '../stories';

const card = (id: string, sender: string, sentAt: number, isMine = false): CardItem => ({
  id,
  sender: { id: sender, displayName: sender.toUpperCase() },
  caption: null,
  altText: null,
  media: null,
  finish: null,
  sentAt,
  isMine,
  status: null,
  hold: null,
  myReaction: null,
});

// Feed order: newest first.
const FEED = [
  card('m2', 'mai', 40),
  card('b1', 'ben', 30),
  card('mine', 'me', 25, true),
  card('m1', 'mai', 20),
  card('l1', 'linh', 10),
];

describe('storyRings', () => {
  it('rings friends with new cards first, opening their oldest new card; my cards get no ring', () => {
    const rings = storyRings(FEED, { ben: 30 });
    expect(rings.map((r) => [r.senderId, r.unseen, r.openId, r.count])).toEqual([
      ['mai', true, 'm1', 2],
      ['linh', true, 'l1', 1],
      ['ben', false, 'b1', 1],
    ]);
  });

  it('opens the next new card once the first one was seen', () => {
    expect(storyRings(FEED, { mai: 20 })[0]).toMatchObject({ senderId: 'mai', openId: 'm2' });
  });
});

describe('storySequence', () => {
  it('plays each friend oldest card first, friends ordered by their newest card', () => {
    expect(storySequence(FEED, 'b1')).toEqual(['m1', 'm2', 'b1', 'l1']);
  });

  it('plays a card that is not a friend card of the loaded feed alone', () => {
    expect(storySequence(FEED, 'mine')).toEqual(['mine']);
    expect(storySequence(FEED, 'elsewhere')).toEqual(['elsewhere']);
  });
});

describe('seen marks', () => {
  it('only moves a mark forward and keeps the newest senders', () => {
    expect(withSeen({ a: 10 }, 'a', 5)).toEqual({ a: 10 });
    let many = {};
    for (let i = 0; i < SEEN_MAX_SENDERS + 5; i++) many = withSeen(many, `s${i}`, i);
    expect(Object.keys(many)).toHaveLength(SEEN_MAX_SENDERS);
    expect(many).not.toHaveProperty('s0');
    expect(isUnseen({ a: 10 }, 'a', 11)).toBe(true);
    expect(isUnseen({ a: 10 }, 'a', 10)).toBe(false);
    expect(isUnseen({}, 'b', 0)).toBe(true);
  });

  it('drops malformed stored data', () => {
    expect(parseSeen('not json')).toEqual({});
    expect(parseSeen('[1]')).toEqual({});
    expect(parseSeen('{"ok":5,"bad id!":3,"nan":"x"}')).toEqual({ ok: 5 });
  });

  it('persists marks under the 2 KB secure-store limit and clears them', async () => {
    const kv = memoryKeyValueStore();
    const store = createSeenStore(kv);
    for (let i = 0; i < 40; i++) {
      await store.markSeen(`00000000-0000-4000-8000-0000000000${10 + i}`, 1e12 + i);
    }
    expect(new TextEncoder().encode(kv.items.get(SEEN_KEY)!).length).toBeLessThan(2048);
    expect(Object.keys(await createSeenStore(kv).load())).toHaveLength(SEEN_MAX_SENDERS);
    await store.clear();
    expect(kv.items.has(SEEN_KEY)).toBe(false);
    expect(await store.load()).toEqual({});
  });
});
