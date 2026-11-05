import { ReduceMotion, withSpring, withTiming } from 'react-native-reanimated';

import { motion, spring } from '@card-sharing/tokens';

export type SpringName = keyof typeof spring;

/**
 * Animates to `toValue` with a design-system spring, or with a short timing when the user asked for
 * reduced motion (design-system.md §6: "springs → 200 ms timing"). Callable from JS or a worklet.
 */
export function animateTo(toValue: number, name: SpringName, reducedMotion: boolean) {
  'worklet';
  if (reducedMotion) {
    return withTiming(toValue, {
      duration: motion.reducedDuration,
      reduceMotion: ReduceMotion.Never,
    });
  }
  return withSpring(toValue, { ...spring[name], reduceMotion: ReduceMotion.System });
}
