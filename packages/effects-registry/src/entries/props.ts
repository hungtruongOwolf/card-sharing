import type { Anchor, EffectEntry } from '../schema';
import { APP_MIN_ANDROID, APP_MIN_IOS } from './platforms';

/**
 * The crown and props family (effects-catalog.md §3.1 #6): vector props drawn by the app (no bitmap assets, so no
 * art licence question), placed on a head anchor and draggable afterwards. With native landmarks or pose
 * (T-115..T-117) they snap to the face; without them the mask heuristic places them and the user adjusts
 * (`userPoints`). `x`/`y` is the prop's anchor point (the bottom centre for hats, the centre otherwise), `size` its
 * width as a share of the card width (auto-placement scales it to the head), `angle` its tilt.
 */
function prop(
  slug: string,
  anchor: Anchor,
  size: number,
  conflictsWith: string[] = [],
): EffectEntry {
  const key = slug.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
  return {
    id: `prop.${slug}`,
    version: 1,
    name: `effects.prop.${key}.name`,
    description: `effects.prop.${key}.description`,
    a11yLabel: `effects.prop.${key}.a11y`,
    kind: 'prop',
    tags: [],
    tier: '0',
    inputs: ['image', 'faceLandmarks', 'animalPose', 'touch'],
    anchor,
    params: {
      size: { type: 'number', min: 0.08, max: 1, default: size, ui: 'slider' },
      angle: { type: 'number', min: -45, max: 45, default: 0, ui: 'slider' },
      x: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
      y: { type: 'number', min: 0, max: 1, default: 0.3, ui: 'hidden' },
    },
    renderer: { type: 'anchoredAsset', ref: `prop:${slug}` },
    layer: 'over',
    stackable: false,
    conflictsWith,
    platforms: {
      ios: { min: APP_MIN_IOS, requires: ['faceLandmarks'], fallback: 'userPoints' },
      android: { min: APP_MIN_ANDROID, requires: ['faceLandmarks'], fallback: 'userPoints' },
      webPreview: 'baked',
    },
    cost: { gpu: 'S', bakeMs: 10, liveAllowed: false, maxLive: 0 },
    export: { still: true, loop: null },
    motion: { reduced: 'static' },
    unlock: 'free',
    flags: {},
  };
}

/** Both sit on top of the head, so they replace each other. */
export const propCrown = prop('crown', 'top', 0.34, ['prop.party-hat']);
export const propPartyHat = prop('party-hat', 'top', 0.26);
export const propSunglasses = prop('sunglasses', 'eyes', 0.36);
export const propMustache = prop('mustache', 'mouth', 0.22);

export const PROPS: readonly EffectEntry[] = [
  propCrown,
  propPartyHat,
  propSunglasses,
  propMustache,
];
