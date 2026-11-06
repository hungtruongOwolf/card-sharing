import type { EffectRenderers } from './effectRenderers';
import { paperLift } from './renderers/paperLift';
import { polaroid } from './renderers/polaroid';
import { PROP_RENDERERS } from './renderers/props';
import { sparklesRenderer } from './renderers/sparkles';
import { speech } from './renderers/speech';
import { washi } from './renderers/washi';

/**
 * The shipped effect renderers by `renderer.ref`. A registry entry whose ref is missing here is never offered
 * (`hasEffectRenderer`); adding an effect = its entry + one line here + tests.
 */
export const EFFECT_RENDERERS: EffectRenderers = {
  effects: {
    'shadow:paperLift': paperLift,
    'tape:washi': washi,
    'text:speech': speech,
    'particles:sparkles': sparklesRenderer,
    ...PROP_RENDERERS,
  },
  frames: {
    'frame:polaroid': polaroid,
  },
};
