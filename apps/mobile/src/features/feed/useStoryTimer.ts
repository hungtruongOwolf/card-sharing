import { useEffect, useRef } from 'react';
import {
  cancelAnimation,
  Easing,
  ReduceMotion,
  useSharedValue,
  withTiming,
  type SharedValue,
} from 'react-native-reanimated';

export interface StoryTimerOptions {
  /** Changing it starts a new segment from zero (the next card). */
  resetKey: string;
  /** Counts down only while true (card ready, not held, no screen reader, no sheet on top). */
  running: boolean;
  durationMs: number;
  onDone: () => void;
  /** Clock seam for tests. */
  now?: () => number;
}

/**
 * One story segment: a JS timer decides when to move on (testable with fake timers) and a shared value drives the
 * progress bar on the UI thread. Pausing keeps the elapsed time; resuming finishes the rest. The bar is a timer, not
 * decoration, so it keeps moving under reduced motion.
 */
export function useStoryTimer({
  resetKey,
  running,
  durationMs,
  onDone,
  now = Date.now,
}: StoryTimerOptions): SharedValue<number> {
  const progress = useSharedValue(0);
  const elapsed = useRef(0);
  const done = useRef(onDone);
  useEffect(() => {
    done.current = onDone;
  });

  // Declared first: on a new card the previous segment's cleanup runs, then this reset, then the countdown.
  useEffect(() => {
    elapsed.current = 0;
    progress.set(0);
  }, [resetKey, progress]);

  useEffect(() => {
    if (!running) return;
    const remaining = Math.max(0, durationMs - elapsed.current);
    const startedAt = now();
    progress.set(
      withTiming(1, {
        duration: remaining,
        easing: Easing.linear,
        reduceMotion: ReduceMotion.Never,
      }),
    );
    const timer = setTimeout(() => done.current(), remaining);
    return () => {
      clearTimeout(timer);
      elapsed.current += now() - startedAt;
      cancelAnimation(progress);
    };
    // `now` is a stable seam.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running, resetKey, durationMs, progress]);

  return progress;
}
