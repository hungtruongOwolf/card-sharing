import { Skia, type SkImage } from '@shopify/react-native-skia';
import { useEffect, useMemo, useState } from 'react';
import { PixelRatio } from 'react-native';

import type { Mask } from '@card-sharing/canvas-engine';
import type { Registry } from '@card-sharing/effects-registry';

import type { CutScene } from '../cut-styles';
import { downscalePhoto, drawCard, subjectFor } from './cardScene';
import { styleChoice, type CardDraft } from './editorState';
import type { PickerTab } from './pickerTabs';

/** Thumbnail width in points (design-system "Effect tile"); the height follows the photo. */
export const THUMB_WIDTH = 56;
/** Pixel density cap for baked thumbnails: sharp on 3x screens without baking huge images. */
const MAX_DENSITY = 3;

export interface ThumbSize {
  width: number;
  height: number;
}

/** Thumbnail size in points for a photo of `aspect` (width / height), never taller than wide x 1.5. */
export function thumbSize(aspect: number): ThumbSize {
  const safe = Number.isFinite(aspect) && aspect > 0 ? aspect : 1;
  return {
    width: THUMB_WIDTH,
    height: Math.round(Math.min(THUMB_WIDTH * 1.5, THUMB_WIDTH / safe)),
  };
}

/** The scene thumbnails are drawn in: the photo and subject at thumbnail pixel size. */
export function thumbScene(image: SkImage, mask: Mask | null, density: number): CutScene {
  const size = thumbSize(image.width() / image.height());
  const scale = Math.min(MAX_DENSITY, Math.max(1, density));
  const width = Math.round(size.width * scale);
  const height = Math.round(size.height * scale);
  return {
    width,
    height,
    photo: downscalePhoto(image, Math.max(width, height) * 2),
    subject: mask ? subjectFor(mask, width) : null,
  };
}

/**
 * Bakes the card a draft describes into a PNG data URI (a raster surface, no live canvas per tile). Null when
 * Skia cannot make a surface; the tile then shows a plain placeholder.
 */
export function bakeThumbnail(
  registry: Registry,
  draft: CardDraft,
  scene: CutScene,
): string | null {
  const surface = Skia.Surface.Make(scene.width, scene.height);
  if (!surface) return null;
  drawCard(surface.getCanvas(), registry, draft, scene);
  surface.flush();
  return `data:image/png;base64,${surface.makeImageSnapshot().encodeToBase64()}`;
}

/** The draft a tile previews: the current card with that tile's entry (default params) swapped in. */
export function tileDraft(tab: PickerTab, entryId: string, shown: CardDraft): CardDraft | null {
  const entry = tab.tiles.find((tile) => tile.entry.id === entryId)?.entry;
  if (!entry) return null;
  switch (tab.slot) {
    case 'cut':
      return { ...shown, cut: styleChoice(entry), caption: '' };
    case 'finish':
      return { ...shown, finish: styleChoice(entry), caption: '' };
    case 'effects':
      // The effect alone on the current card, so each tile shows what it adds.
      return { ...shown, effects: [styleChoice(entry)], caption: '' };
  }
}

/**
 * Baked previews of the active tab's tiles on the current photo, keyed by entry id. Baked after the first paint
 * (an effect), so opening a tab never waits on them; a tile shows a placeholder until its thumbnail is ready.
 * Unavailable tiles get none (they cannot draw on this card). Re-bakes only when what the tiles show changes:
 * the whole-photo choice and the other slots (cut tiles keep the finish, finish tiles the cut, effect tiles both).
 */
export function useTileThumbnails(
  registry: Registry,
  tab: PickerTab,
  shown: CardDraft | null,
  image: SkImage,
  mask: Mask | null,
): Readonly<Record<string, string>> {
  const scene = useMemo(() => thumbScene(image, mask, PixelRatio.get()), [image, mask]);
  const context = shown
    ? JSON.stringify({
        slot: tab.slot,
        cut: tab.slot === 'cut' ? null : shown.cut,
        finish: tab.slot === 'finish' ? null : shown.finish,
        wholePhoto: shown.wholePhoto,
      })
    : '';
  const ids = tab.tiles
    .filter((tile) => tile.state.kind !== 'unavailable')
    .map((tile) => tile.entry.id)
    .join(',');
  const [baked, setBaked] = useState<{ key: string; uris: Record<string, string> }>({
    key: '',
    uris: {},
  });
  const key = `${context}|${ids}`;

  useEffect(() => {
    if (!shown) return undefined;
    // Next macrotask: the tab paints first, and a quick change of selection cancels a stale bake.
    const handle = setTimeout(() => {
      const uris: Record<string, string> = {};
      for (const id of ids ? ids.split(',') : []) {
        const draft = tileDraft(tab, id, shown);
        const uri = draft ? bakeThumbnail(registry, draft, scene) : null;
        if (uri) uris[id] = uri;
      }
      setBaked({ key, uris });
    }, 0);
    return () => clearTimeout(handle);
    // `key` captures everything the thumbnails depend on besides the scene and registry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, scene, registry]);

  return baked.key === key ? baked.uris : {};
}
