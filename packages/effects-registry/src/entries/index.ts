import type { EffectEntry } from '../schema';
import { cutDiecut } from './cut.diecut';
import { cutNone } from './cut.none';
import { cutRaw } from './cut.raw';
import { fxHolo } from './fx.holo';
import { fxPaperLift } from './fx.paper-lift';

/**
 * Every shipped entry, in picker order. Adding an effect = one entry file + one line here + tests
 * (AC-10.2); screens read this list and never name ids.
 */
export const ENTRIES: readonly EffectEntry[] = [cutNone, cutRaw, cutDiecut, fxHolo, fxPaperLift];
