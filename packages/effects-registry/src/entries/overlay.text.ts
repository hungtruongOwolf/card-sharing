import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Text sticker (sprint 9, Stories-style text): a short phrase in one of the brand faces with a style (plain with a
 * soft shadow, a background pill, an outline, a neon glow), a colour, a size and a tilt, dragged anywhere. The words
 * come from a curated library like the speech captions: no free text is baked into the image in the MVP (the free
 * caption stays separate, filtered metadata).
 */
export const TEXT_PHRASES = [
  'omg',
  'love',
  'bff',
  'yay',
  'mood',
  'soCute',
  'goodVibes',
  'hbd',
  'missYou',
  'wow',
  'iconic',
  'mainCharacter',
  'weekend',
  'thankYou',
] as const;

/** Brand roles a text sticker can use (design-system type roles; the app maps them to faces). */
export const TEXT_FONTS = ['display', 'hand', 'meme', 'mono', 'body'] as const;
export const TEXT_STYLES = ['plain', 'pill', 'outline', 'neon'] as const;

export const overlayText: EffectEntry = {
  id: 'overlay.text',
  version: 1,
  name: 'effects.overlay.text.name',
  description: 'effects.overlay.text.description',
  a11yLabel: 'effects.overlay.text.a11y',
  kind: 'overlay',
  tags: ['text', 'new'],
  tier: '0',
  inputs: ['text', 'touch'],
  anchor: 'none',
  params: {
    phrase: { type: 'enum', values: TEXT_PHRASES, default: 'omg', ui: 'segmented' },
    font: { type: 'enum', values: TEXT_FONTS, default: 'display', ui: 'segmented' },
    textStyle: { type: 'enum', values: TEXT_STYLES, default: 'pill', ui: 'segmented' },
    color: { type: 'color', default: '#FFFFFF', ui: 'color' },
    size: { type: 'number', min: 0.5, max: 2.5, default: 1, ui: 'slider' },
    angle: { type: 'number', min: -45, max: 45, default: -4, ui: 'slider' },
    x: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
    y: { type: 'number', min: 0, max: 1, default: 0.2, ui: 'hidden' },
  },
  renderer: { type: 'vector', ref: 'text:sticker' },
  layer: 'over',
  stackable: true,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 10, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
