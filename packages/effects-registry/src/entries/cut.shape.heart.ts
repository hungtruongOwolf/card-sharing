import type { EffectEntry } from '../schema';
import { SHAPE_BORDER_PARAMS, SHAPE_CUT_BASE } from './shapeCut';

/** The photo in a centred heart. */
export const cutShapeHeart: EffectEntry = {
  ...SHAPE_CUT_BASE,
  id: 'cut.shape.heart',
  name: 'effects.cut.shape.heart.name',
  description: 'effects.cut.shape.heart.description',
  a11yLabel: 'effects.cut.shape.heart.a11y',
  params: { ...SHAPE_BORDER_PARAMS },
  renderer: { type: 'pathEffect', ref: 'shape:heart' },
};
