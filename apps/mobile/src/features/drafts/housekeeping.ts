import type { LocalCard } from '../cards/localCard';

/** Drafts not edited for this long are offered for cleanup (prd-mvp edge cases: low storage). */
export const STALE_DRAFT_DAYS = 30;
/** Below this much free space the tray warns (prd-mvp: warn < 500 MB free). */
export const LOW_STORAGE_BYTES = 500 * 1024 * 1024;

const DAY_MS = 24 * 60 * 60 * 1000;

export function draftAgeDays(card: LocalCard, now: Date): number {
  return Math.max(0, Math.floor((now.getTime() - Date.parse(card.updatedAt)) / DAY_MS));
}

export function staleDrafts(cards: readonly LocalCard[], now: Date): LocalCard[] {
  return cards.filter((card) => draftAgeDays(card, now) > STALE_DRAFT_DAYS);
}

/** Free-space probe seam. `null` = unknown (no warning). */
export interface StorageInfo {
  freeBytes(): Promise<number | null>;
}

/**
 * No free-space API is a direct dependency yet, so the warning stays off until
 * the follow-up adds a real probe behind this seam.
 */
export const unknownStorageInfo: StorageInfo = { freeBytes: () => Promise.resolve(null) };

export const isLowStorage = (freeBytes: number | null): boolean =>
  freeBytes !== null && freeBytes < LOW_STORAGE_BYTES;
