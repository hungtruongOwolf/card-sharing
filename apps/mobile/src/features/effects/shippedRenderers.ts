import type { EffectRenderers } from './effectRenderers';
import { paperLift } from './renderers/paperLift';
import { polaroid } from './renderers/polaroid';

/**
 * The shipped effect renderers by `renderer.ref`. A registry entry whose ref is missing here is never offered
 * (`hasEffectRenderer`); adding an effect = its entry + one line here + tests.
 */
export const EFFECT_RENDERERS: EffectRenderers = {
  effects: {
    'shadow:paperLift': paperLift,
  },
  frames: {
    'frame:polaroid': polaroid,
  },
};
