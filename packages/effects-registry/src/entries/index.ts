import type { EffectEntry } from '../schema';
import { BACKDROPS, DECORATIONS } from './board';
import { cutDiecut } from './cut.diecut';
import { cutNone } from './cut.none';
import { cutRaw } from './cut.raw';
import { cutShapeCircle } from './cut.shape.circle';
import { cutShapeHeart } from './cut.shape.heart';
import { cutShapeRoundedSquare } from './cut.shape.rounded-square';
import { cutTorn } from './cut.torn';
import { framePolaroid } from './frame.polaroid';
import { fxFireBreath } from './fx.fire-breath';
import { fxGlossy } from './fx.glossy';
import { fxHolo } from './fx.holo';
import { fxLaserEyes } from './fx.laser-eyes';
import { fxMatte } from './fx.matte';
import { fxPaperLift } from './fx.paper-lift';
import { fxSparkles } from './fx.sparkles';
import { overlaySpeech } from './overlay.speech';
import { overlayText } from './overlay.text';
import { overlayWashi } from './overlay.washi';
import { PROPS } from './props';

/**
 * Every shipped entry, in picker order. Adding an effect = one entry file + one line here + tests
 * (AC-10.2); screens read this list and never name ids.
 */
export const ENTRIES: readonly EffectEntry[] = [
  cutNone,
  cutRaw,
  cutDiecut,
  cutTorn,
  cutShapeRoundedSquare,
  cutShapeCircle,
  cutShapeHeart,
  fxMatte,
  fxGlossy,
  fxHolo,
  fxPaperLift,
  framePolaroid,
  overlayWashi,
  overlaySpeech,
  overlayText,
  fxSparkles,
  ...PROPS,
  fxLaserEyes,
  fxFireBreath,
  ...BACKDROPS,
  ...DECORATIONS,
];
