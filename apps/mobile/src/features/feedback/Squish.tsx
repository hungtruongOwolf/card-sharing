import { useMemo, type ReactNode } from 'react';
import type { ViewProps } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  ReduceMotion,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type SharedValue,
} from 'react-native-reanimated';

import { motion } from '@card-sharing/tokens';

import { animateTo, useTheme } from '../../ui';
import { useFeedback } from './FeedbackProvider';

/** Press-in shape at a normal tap (design-system.md §5.2). */
export const SQUISH_SHAPE = { x: 1.04, y: 0.94 } as const;
/** Fast repeated taps squish deeper, never below this scaleY (design-system.md §5.2: capped at 0.88). */
export const SQUISH_MIN_SCALE_Y = 0.88;
/** Taps closer together than this count as a streak. */
export const SQUISH_STREAK_MS = 350;

/**
 * Press depth for a tap: 1 for a single tap, growing with the tap rate up to the cap. Tap rate stands in for
 * touch force, which most Android phones do not report.
 */
export function squishDepth(streak: number): number {
  const max = (1 - SQUISH_MIN_SCALE_Y) / (1 - SQUISH_SHAPE.y);
  return Math.min(max, 1 + Math.max(0, streak) * 0.25);
}

/** Wall-clock for tap streaks (outside the component: reading a clock is not a render concern). */
const tapClock = () => Date.now();

export interface SquishProps extends Pick<ViewProps, 'style' | 'testID'> {
  children: ReactNode;
  /** Optional shared press value (0..depth) for effects that react to it, e.g. the holo tilt bump. */
  press?: SharedValue<number>;
  /** Gesture test id (react-native-gesture-handler jest utils). */
  gestureTestId?: string;
}

/**
 * Squishy card (T-66, US-11): press-in squash, `spring.squish` rebound, a haptic tick and a squeak through the
 * feedback events. Squish is a view transform, so the card never repaints (AC-11.2 budget). Reduced motion:
 * an opacity dip instead of scaling. Children keep their own accessibility (the squish is touch-only flair).
 */
export function Squish({ children, press: external, style, testID, gestureTestId }: SquishProps) {
  const { reducedMotion } = useTheme();
  const { emit } = useFeedback();
  const own = useSharedValue(0);
  const press = external ?? own;
  const lastTap = useSharedValue(-Infinity);
  const streak = useSharedValue(0);

  // Memoized: a gesture rebuilt on every render re-registers its handler (and its test id) each time.
  const gesture = useMemo(() => {
    const tap = Gesture.Tap()
      .maxDuration(10_000)
      .runOnJS(true)
      .onBegin(() => {
        const at = tapClock();
        const next = at - lastTap.get() < SQUISH_STREAK_MS ? streak.get() + 1 : 0;
        streak.set(next);
        lastTap.set(at);
        press.set(
          reducedMotion
            ? animateTo(1, 'snappy', true)
            : withTiming(squishDepth(next), {
                duration: motion.duration.xs,
                reduceMotion: ReduceMotion.Never,
              }),
        );
        emit('squish');
      })
      .onFinalize(() => press.set(animateTo(0, 'squish', reducedMotion)));
    return gestureTestId ? tap.withTestId(gestureTestId) : tap;
  }, [press, lastTap, streak, reducedMotion, emit, gestureTestId]);

  const animated = useAnimatedStyle(() => {
    const p = press.get();
    if (reducedMotion) return { opacity: 1 - Math.min(1, p) * 0.2 };
    return {
      transform: [
        { scaleX: 1 + (SQUISH_SHAPE.x - 1) * p },
        { scaleY: Math.max(SQUISH_MIN_SCALE_Y, 1 + (SQUISH_SHAPE.y - 1) * p) },
      ],
    };
  });

  return (
    <GestureDetector gesture={gesture}>
      <Animated.View style={[style, animated]} testID={testID}>
        {children}
      </Animated.View>
    </GestureDetector>
  );
}
