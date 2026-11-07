import type { Anchor, EffectEntry } from '../schema';
import { NEEDS_FACE_LANDMARKS } from './platforms';

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
  extraTags: string[] = [],
): EffectEntry {
  const key = slug.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
  return {
    id: `prop.${slug}`,
    version: 1,
    name: `effects.prop.${key}.name`,
    description: `effects.prop.${key}.description`,
    a11yLabel: `effects.prop.${key}.a11y`,
    kind: 'prop',
    // `sticker`: listed by the editor's Stickers tool too (sprint 9).
    tags: ['sticker', ...extraTags],
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
    platforms: NEEDS_FACE_LANDMARKS,
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

/** Sprint 9 additions: more head-top props (one per head) and heart glasses (one pair per face). */
const HATS = ['prop.crown', 'prop.party-hat'];
export const propHalo = prop('halo', 'top', 0.36, [...HATS], ['new']);
export const propBow = prop('bow', 'top', 0.3, [...HATS, 'prop.halo'], ['new']);
export const propCatEars = prop(
  'cat-ears',
  'top',
  0.5,
  [...HATS, 'prop.halo', 'prop.bow'],
  ['new'],
);
export const propHeartGlasses = prop('heart-glasses', 'eyes', 0.38, ['prop.sunglasses'], ['new']);

export const PROPS: readonly EffectEntry[] = [
  propCrown,
  propPartyHat,
  propSunglasses,
  propMustache,
  propHalo,
  propBow,
  propCatEars,
  propHeartGlasses,
];
