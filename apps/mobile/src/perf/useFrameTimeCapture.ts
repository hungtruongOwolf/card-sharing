import { useCallback, useEffect, useState } from 'react';
import { useFrameCallback, useSharedValue, type FrameInfo } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';

import {
  createFrameHistogram,
  recordFrame,
  resetFrameHistogram,
  summarizeFrames,
  type FrameHistogram,
  type FrameStats,
} from './frameStats';

export interface FrameTimeReport {
  /** The last `reportEveryMs` window. */
  live: FrameStats;
  /** Everything since the capture started or was last reset. */
  session: FrameStats;
}

export interface FrameTimeCaptureOptions {
  enabled: boolean;
  /** Frame rate the dropped-frame count is measured against (the 60 fps budget, design-system.md §7). */
  targetFps?: number;
  /** How often the UI thread hands a summary to JS. Once per second keeps JS out of the frame loop. */
  reportEveryMs?: number;
}

interface CaptureState {
  live: FrameHistogram;
  session: FrameHistogram;
  sinceReportMs: number;
  /** Set from JS; the UI thread clears both histograms on the next frame. */
  resetRequested: boolean;
}

/**
 * Records the interval between UI-thread frames (Reanimated frame callback, i.e. the display link /
 * Choreographer tick that drives our worklets) and reports p50/p95/p99 and dropped frames.
 *
 * Measures what the UI thread delivers: a long GPU frame shows up as a late next callback, but a
 * frame the compositor drops after the UI thread finished does not. Read it next to the OS tools
 * (Android `dumpsys gfxinfo`, Xcode Instruments) as described in the T-52 write-up.
 */
export function useFrameTimeCapture({
  enabled,
  targetFps = 60,
  reportEveryMs = 1000,
}: FrameTimeCaptureOptions) {
  const [report, setReport] = useState<FrameTimeReport | null>(null);
  const state = useSharedValue<CaptureState>({
    live: createFrameHistogram(),
    session: createFrameHistogram(),
    sinceReportMs: 0,
    resetRequested: false,
  });

  const onFrame = useCallback(
    ({ timeSincePreviousFrame }: FrameInfo) => {
      'worklet';
      if (timeSincePreviousFrame === null) return;
      state.modify((s) => {
        'worklet';
        if (s.resetRequested) {
          resetFrameHistogram(s.live);
          resetFrameHistogram(s.session);
          s.sinceReportMs = 0;
          s.resetRequested = false;
          return s;
        }
        recordFrame(s.live, timeSincePreviousFrame, targetFps);
        recordFrame(s.session, timeSincePreviousFrame, targetFps);
        s.sinceReportMs += timeSincePreviousFrame;
        if (s.sinceReportMs >= reportEveryMs) {
          const summary: FrameTimeReport = {
            live: summarizeFrames(s.live),
            session: summarizeFrames(s.session),
          };
          resetFrameHistogram(s.live);
          s.sinceReportMs = 0;
          scheduleOnRN(setReport, summary);
        }
        return s;
      }, false);
    },
    [state, targetFps, reportEveryMs],
  );

  const frameCallback = useFrameCallback(onFrame, false);
  useEffect(() => {
    frameCallback.setActive(enabled);
  }, [frameCallback, enabled]);

  const reset = useCallback(() => {
    state.modify((s) => {
      'worklet';
      s.resetRequested = true;
      return s;
    }, false);
    setReport(null);
  }, [state]);

  return { report, reset };
}
