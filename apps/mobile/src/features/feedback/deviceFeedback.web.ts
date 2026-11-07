import { noopHaptics, noopSound, type HapticsPort, type SoundPort } from './feedback';

/**
 * Web (ADR-0009): no haptics (browsers have no haptics engine; `navigator.vibrate` is not the design's feel) and no
 * touch sounds in the preview build.
 */
export function deviceFeedback(): { haptics: HapticsPort; sound: SoundPort } {
  return { haptics: noopHaptics, sound: noopSound };
}
