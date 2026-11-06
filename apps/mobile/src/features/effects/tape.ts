/**
 * Physical washi tape tones (design-system.md §2 washi: semi-transparent, 85 % alpha), the same in both colour
 * schemes. Shared by the card tape (`overlay.washi`) and the board tape decorations (T-111); the keys are the
 * registry's `TAPE_COLORS`.
 */
export const TAPE_PALETTE: Readonly<Record<string, string>> = {
  mustard: '#E8B53A',
  sky: '#8EC5E8',
  mint: '#9ED9B8',
  coral: '#F08C7A',
  lilac: '#C3A8E6',
};
export const TAPE_ALPHA = 0.85;

/** The tone of a `tapeColor` param; unknown values fall back to mustard. */
export function tapeTone(color: unknown): string {
  return TAPE_PALETTE[typeof color === 'string' ? color : ''] ?? TAPE_PALETTE.mustard!;
}
