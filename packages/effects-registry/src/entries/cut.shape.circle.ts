import type { EffectEntry } from '../schema';
import { SHAPE_BORDER_PARAMS, SHAPE_CUT_BASE } from './shapeCut';

/** Round badge: the photo in a centred circle. */
export const cutShapeCircle: EffectEntry = {
  ...SHAPE_CUT_BASE,
  id: 'cut.shape.circle',
  name: 'effects.cut.shape.circle.name',
  description: 'effects.cut.shape.circle.description',
  a11yLabel: 'effects.cut.shape.circle.a11y',
  params: { ...SHAPE_BORDER_PARAMS },
  renderer: { type: 'pathEffect', ref: 'shape:circle' },
};
