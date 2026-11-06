/** A card the enlarge dialog can show: the baked, watermarked still from the snapshot. */
export type EnlargeCard = {
  readonly src: string;
  readonly width: number;
  readonly height: number;
  readonly label: string;
};

/** Attribute on each card button that holds its position in the `cards` list. */
export const CARD_ATTRIBUTE = 'data-card';

/** Reads a card button's position; `null` for anything that is not a valid index. */
export function cardIndex(value: string | null | undefined, count: number): number | null {
  if (value === null || value === undefined || !/^\d{1,3}$/.test(value)) return null;
  const index = Number(value);
  return index < count ? index : null;
}

/** Accessible name of a card, by its 0-based position. */
export function cardLabel(index: number, count: number): string {
  return `Card ${index + 1} of ${count}`;
}
