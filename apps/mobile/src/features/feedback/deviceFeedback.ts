import type { HapticsPort, SoundPort } from './feedback';
import { createExpoHaptics } from './hapticsAdapter';
import { createExpoSound } from './soundAdapter';

/**
 * The device's feedback engines (#246): expo-haptics and expo-audio. The web build resolves `deviceFeedback.web.ts`
 * (no-op engines).
 */
export function deviceFeedback(): { haptics: HapticsPort; sound: SoundPort } {
  return { haptics: createExpoHaptics(), sound: createExpoSound() };
}
