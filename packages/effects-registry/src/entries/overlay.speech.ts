import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Comic speech bubble or meme caption (effects-catalog.md §3.1 #14). The text comes from a curated caption library
 * (`caption`, no free text in the MVP), so nothing user-written is baked into the image. The bubble's tail points at
 * `tailX`/`tailY`: the mouth or head anchor when one is known, else the top of the subject or a point the user drags
 * (the manual fallback). `x`/`y` place the bubble or caption.
 */
export const SPEECH_CAPTIONS = [
  'hello',
  'lol',
  'omg',
  'mood',
  'nope',
  'yum',
  'zoomies',
  'feedMe',
  'mine',
  'bestDay',
] as const;

export const overlaySpeech: EffectEntry = {
  id: 'overlay.speech',
  version: 1,
  name: 'effects.overlay.speech.name',
  description: 'effects.overlay.speech.description',
  a11yLabel: 'effects.overlay.speech.a11y',
  kind: 'overlay',
  // Listed by the editor's Text tool too (sprint 9).
  tags: ['text'],
  tier: '0',
  inputs: ['text', 'touch', 'faceLandmarks', 'animalPose'],
  anchor: 'mouth',
  params: {
    caption: { type: 'enum', values: SPEECH_CAPTIONS, default: 'hello', ui: 'segmented' },
    shape: { type: 'enum', values: ['bubble', 'meme'], default: 'bubble', ui: 'segmented' },
    size: { type: 'number', min: 0.6, max: 1.6, default: 1, ui: 'slider' },
    x: { type: 'number', min: 0, max: 1, default: 0.7, ui: 'hidden' },
    y: { type: 'number', min: 0, max: 1, default: 0.15, ui: 'hidden' },
    tailX: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
    tailY: { type: 'number', min: 0, max: 1, default: 0.4, ui: 'hidden' },
  },
  renderer: { type: 'vector', ref: 'text:speech' },
  layer: 'over',
  stackable: true,
  conflictsWith: [],
  platforms: {
    // Without landmarks or pose the tail is placed by the subject heuristic or by hand (`userPoints`).
    ios: { min: ALL_DEVICES.ios.min, requires: ['faceLandmarks'], fallback: 'userPoints' },
    android: { min: ALL_DEVICES.android.min, requires: ['faceLandmarks'], fallback: 'userPoints' },
    webPreview: 'baked',
  },
  cost: { gpu: 'S', bakeMs: 10, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
