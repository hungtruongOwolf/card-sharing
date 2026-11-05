import type { EffectEntry } from '../schema';
import { SHAPE_BORDER_PARAMS, SHAPE_CUT_BASE } from './shapeCut';

/** Square crop with soft corners; `cornerRadiusPct` of the side. */
export const cutShapeRoundedSquare: EffectEntry = {
  ...SHAPE_CUT_BASE,
  id: 'cut.shape.rounded-square',
  name: 'effects.cut.shape.roundedSquare.name',
  description: 'effects.cut.shape.roundedSquare.description',
  a11yLabel: 'effects.cut.shape.roundedSquare.a11y',
  params: {
    cornerRadiusPct: { type: 'number', min: 4, max: 30, default: 18, ui: 'slider' },
    ...SHAPE_BORDER_PARAMS,
  },
  renderer: { type: 'pathEffect', ref: 'shape:roundedSquare' },
};
