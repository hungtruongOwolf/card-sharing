import type { Registry } from '@card-sharing/effects-registry';

import { loadImageFromUri, type LoadImage } from '../editor/CardEditorScreen';
import type { MaskSource } from '../editor/maskSource';
import { loadEffectFonts } from '../effects';
import { bakeCard, BakeError, encodeOriginal, type BakedImage } from './bake';
import type { LocalCard } from './localCard';

/** What the send flow uploads for one card (T-77): never the raw capture file. */
export interface PreparedCard {
  render: BakedImage;
  /** Only when the cut style reads the mask (see `usesMask`). */
  mask: BakedImage | null;
  /** Only with "keep original" on (AC-13.4). */
  original: BakedImage | null;
}

export interface PrepareDeps {
  registry: Registry;
  maskSource: MaskSource;
  loadImage?: LoadImage;
  /** Registry ids switched off remotely, so the bake matches the editor preview. */
  killed?: ReadonlySet<string>;
  /** Loads the fonts text effects draw with before the bake (tests inject one). */
  loadFonts?: () => Promise<void>;
}

/**
 * Bakes a local card for sending, on the device: decodes the full-resolution photo, asks the mask source again
 * (masks are never stored), renders the card at 1080 px and, only when asked, re-encodes the original. Runs when
 * the user taps Send, never earlier; nothing here talks to the network.
 */
export async function prepareForSend(
  card: LocalCard,
  keepOriginal: boolean,
  {
    registry,
    maskSource,
    loadImage = loadImageFromUri,
    killed,
    loadFonts = loadEffectFonts,
  }: PrepareDeps,
): Promise<PreparedCard> {
  const [photo, mask] = await Promise.all([
    loadImage(card.photo.uri),
    card.draft.wholePhoto
      ? Promise.resolve(null)
      : maskSource.maskFor(card.photo).catch(() => null),
    loadFonts().catch(() => undefined),
  ]);
  if (!photo) throw new BakeError('photo unavailable');
  const baked = bakeCard({ registry, draft: card.draft, photo, mask, purpose: 'send', killed });
  return { ...baked, original: keepOriginal ? encodeOriginal(photo) : null };
}
