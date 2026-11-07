import { secureKeyValueStore, type KeyValueStore } from '../drafts/draftStore';

/**
 * Which friends' cards I have already opened, for the unseen story ring (sprint 9). Kept as "seen up to" per sender
 * (the `sentAt` of the newest card of theirs I opened), so the copy stays tiny: a card is unseen when it was sent
 * after that mark. Only ids and times are stored, never names or captions; the copy is cleared on sign-out like the
 * rest of the account's device data.
 */
export type SeenMarks = Readonly<Record<string, number>>;

export interface SeenStore {
  load(): Promise<SeenMarks>;
  /** Records that I saw `senderId`'s cards up to `sentAt`; never moves a mark back. Returns the new marks. */
  markSeen(senderId: string, sentAt: number): Promise<SeenMarks>;
  clear(): Promise<void>;
}

export const SEEN_KEY = 'cs.feed.seen.v1';
/** Senders kept (the most recently marked); stays under the 2 KB secure-store value limit. */
export const SEEN_MAX_SENDERS = 30;
const ID = /^[0-9A-Za-z-]{1,64}$/;

/** Parses stored marks, dropping anything malformed. */
export function parseSeen(raw: string | null): SeenMarks {
  if (!raw) return {};
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null || Array.isArray(value)) return {};
    const out: Record<string, number> = {};
    for (const [id, at] of Object.entries(value)) {
      if (ID.test(id) && typeof at === 'number' && Number.isFinite(at)) out[id] = at;
    }
    return out;
  } catch {
    return {};
  }
}

/** Marks with `senderId` moved forward to `sentAt`, keeping the {@link SEEN_MAX_SENDERS} newest marks. */
export function withSeen(marks: SeenMarks, senderId: string, sentAt: number): SeenMarks {
  if ((marks[senderId] ?? -Infinity) >= sentAt) return marks;
  const next = Object.entries({ ...marks, [senderId]: sentAt })
    .sort((a, b) => b[1] - a[1])
    .slice(0, SEEN_MAX_SENDERS);
  return Object.fromEntries(next);
}

/** True when the card was sent after the last one of that sender I opened. */
export const isUnseen = (marks: SeenMarks, senderId: string, sentAt: number): boolean =>
  sentAt > (marks[senderId] ?? -Infinity);

export function createSeenStore(store: KeyValueStore = secureKeyValueStore()): SeenStore {
  let cached: SeenMarks | null = null;
  const load = async () => {
    cached ??= parseSeen(await store.getItem(SEEN_KEY).catch(() => null));
    return cached;
  };
  return {
    load,
    async markSeen(senderId, sentAt) {
      const marks = await load();
      const next = withSeen(marks, senderId, sentAt);
      if (next === marks) return marks;
      cached = next;
      await store.setItem(SEEN_KEY, JSON.stringify(next)).catch(() => undefined);
      return next;
    },
    async clear() {
      cached = {};
      await store.deleteItem(SEEN_KEY).catch(() => undefined);
    },
  };
}

let deviceSeen: SeenStore | null = null;

/** The app's seen store (secure store on the device). */
export function deviceSeenStore(): SeenStore {
  deviceSeen ??= createSeenStore();
  return deviceSeen;
}
