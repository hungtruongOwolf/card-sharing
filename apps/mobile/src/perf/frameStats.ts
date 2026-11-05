/**
 * Frame-interval statistics for the dev frame-time overlay (T-52).
 *
 * Intervals are recorded into a fixed-bin histogram so that recording is O(1) per frame and allocation
 * free on the UI thread, and percentiles never need a sort. Bins are {@link BIN_MS} wide, so percentiles
 * are reported as the upper edge of their bin (at most 0.5 ms pessimistic), capped at the observed max.
 *
 * Every function is a worklet: it runs on the UI thread inside a frame callback, and in Jest as plain JS.
 */

/** Histogram bin width in ms. */
export const BIN_MS = 0.5;
/** Intervals of this many ms or more land in the last (overflow) bin. */
export const MAX_BINNED_MS = 100;
const BIN_COUNT = MAX_BINNED_MS / BIN_MS + 1;

export interface FrameHistogram {
  /** counts[i] = frames whose interval fell in [i · BIN_MS, (i + 1) · BIN_MS); the last bin is overflow. */
  counts: number[];
  frames: number;
  totalMs: number;
  maxMs: number;
  /** Refresh periods missed, estimated against the target frame rate (see {@link missedFrames}). */
  dropped: number;
}

export interface FrameStats {
  frames: number;
  durationMs: number;
  /** Frames per second actually delivered over the window. */
  avgFps: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
  maxMs: number;
  dropped: number;
  /** dropped / (frames + dropped) · 100: share of the frames the target rate expected that never came. */
  droppedPct: number;
}

export function createFrameHistogram(): FrameHistogram {
  'worklet';
  const counts: number[] = [];
  for (let i = 0; i < BIN_COUNT; i += 1) counts.push(0);
  return { counts, frames: 0, totalMs: 0, maxMs: 0, dropped: 0 };
}

export function resetFrameHistogram(h: FrameHistogram): void {
  'worklet';
  for (let i = 0; i < h.counts.length; i += 1) h.counts[i] = 0;
  h.frames = 0;
  h.totalMs = 0;
  h.maxMs = 0;
  h.dropped = 0;
}

/**
 * Refresh periods skipped by one interval at `targetFps`: a 33 ms interval at 60 fps missed one frame.
 * Rounded, so normal vsync jitter (e.g. 17.9 ms) counts as on time; intervals shorter than the period
 * (a 120 Hz panel) count as on time too.
 */
export function missedFrames(intervalMs: number, targetFps: number): number {
  'worklet';
  const period = 1000 / targetFps;
  return Math.max(0, Math.round(intervalMs / period) - 1);
}

/** Records one frame interval. Non-finite or non-positive intervals are ignored. */
export function recordFrame(h: FrameHistogram, intervalMs: number, targetFps: number): void {
  'worklet';
  if (!Number.isFinite(intervalMs) || intervalMs <= 0) return;
  const bin = Math.min(BIN_COUNT - 1, Math.floor(intervalMs / BIN_MS));
  h.counts[bin] = (h.counts[bin] ?? 0) + 1;
  h.frames += 1;
  h.totalMs += intervalMs;
  if (intervalMs > h.maxMs) h.maxMs = intervalMs;
  h.dropped += missedFrames(intervalMs, targetFps);
}

function percentile(h: FrameHistogram, p: number): number {
  'worklet';
  if (h.frames === 0) return 0;
  const rank = Math.ceil(p * h.frames);
  let seen = 0;
  for (let i = 0; i < h.counts.length; i += 1) {
    seen += h.counts[i] ?? 0;
    if (seen >= rank) {
      if (i === h.counts.length - 1) return h.maxMs;
      return Math.min((i + 1) * BIN_MS, h.maxMs);
    }
  }
  return h.maxMs;
}

export function summarizeFrames(h: FrameHistogram): FrameStats {
  'worklet';
  const expected = h.frames + h.dropped;
  return {
    frames: h.frames,
    durationMs: h.totalMs,
    avgFps: h.totalMs > 0 ? (h.frames * 1000) / h.totalMs : 0,
    p50Ms: percentile(h, 0.5),
    p95Ms: percentile(h, 0.95),
    p99Ms: percentile(h, 0.99),
    maxMs: h.maxMs,
    dropped: h.dropped,
    droppedPct: expected > 0 ? (h.dropped / expected) * 100 : 0,
  };
}

/** Convenience for tests and offline analysis: stats of a list of intervals. */
export function statsFromIntervals(intervalsMs: readonly number[], targetFps = 60): FrameStats {
  const h = createFrameHistogram();
  for (const interval of intervalsMs) recordFrame(h, interval, targetFps);
  return summarizeFrames(h);
}
