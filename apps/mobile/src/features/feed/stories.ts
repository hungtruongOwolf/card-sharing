import type { CardItem } from '../sharing/cardsApi';
import { isUnseen, type SeenMarks } from './seenCards';

/** One friend in the story row: who, whether something is new, and which card a tap opens. */
export interface StoryRing {
  senderId: string;
  /** Server display name, or null (the screen shows "A friend"). */
  name: string | null;
  unseen: boolean;
  /** Cards from this friend in the loaded feed. */
  count: number;
  /** The oldest unseen card (Stories order), else the newest one. */
  openId: string;
}

/** Friends' cards grouped by sender, each group oldest first; groups ordered by their newest card. */
function groups(items: readonly CardItem[]): CardItem[][] {
  const bySender = new Map<string, CardItem[]>();
  for (const item of items) {
    if (item.isMine) continue;
    const list = bySender.get(item.sender.id) ?? [];
    list.push(item);
    bySender.set(item.sender.id, list);
  }
  return [...bySender.values()]
    .map((list) => [...list].sort((a, b) => a.sentAt - b.sentAt))
    .sort((a, b) => (b.at(-1)?.sentAt ?? 0) - (a.at(-1)?.sentAt ?? 0));
}

/** The story row: friends with something new first (newest first), then the rest. My own cards get no ring. */
export function storyRings(items: readonly CardItem[], seen: SeenMarks): StoryRing[] {
  const rings = groups(items).map((list): StoryRing => {
    const newest = list.at(-1)!;
    const firstUnseen = list.find((card) => isUnseen(seen, card.sender.id, card.sentAt));
    return {
      senderId: newest.sender.id,
      name: newest.sender.displayName,
      unseen: firstUnseen !== undefined,
      count: list.length,
      openId: (firstUnseen ?? newest).id,
    };
  });
  return [...rings.filter((r) => r.unseen), ...rings.filter((r) => !r.unseen)];
}

/**
 * The order the story viewer plays: each friend's cards oldest to newest, friends in feed order (newest card first).
 * The viewer starts at the opened card, goes back into the previous friend and closes after the last card. A card
 * that is not a friend's card in the loaded feed (my own card, a link from activity) plays alone.
 */
export function storySequence(items: readonly CardItem[], startId: string): string[] {
  const ids = groups(items).flatMap((list) => list.map((card) => card.id));
  return ids.includes(startId) ? ids : [startId];
}
