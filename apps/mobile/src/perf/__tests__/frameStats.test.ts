import {
  BIN_MS,
  createFrameHistogram,
  missedFrames,
  recordFrame,
  resetFrameHistogram,
  statsFromIntervals,
  summarizeFrames,
} from '../frameStats';

const repeat = (value: number, times: number) => Array.from({ length: times }, () => value);

describe('missedFrames', () => {
  it('counts skipped refresh periods at the target rate', () => {
    expect(missedFrames(16.7, 60)).toBe(0);
    expect(missedFrames(17.9, 60)).toBe(0); // vsync jitter is on time
    expect(missedFrames(33.3, 60)).toBe(1);
    expect(missedFrames(50, 60)).toBe(2);
    expect(missedFrames(8.3, 60)).toBe(0); // 120 Hz panel, faster than the target
    expect(missedFrames(16.7, 120)).toBe(1);
  });
});

describe('statsFromIntervals', () => {
  it('returns zeros for an empty capture', () => {
    expect(statsFromIntervals([])).toEqual({
      frames: 0,
      durationMs: 0,
      avgFps: 0,
      p50Ms: 0,
      p95Ms: 0,
      p99Ms: 0,
      maxMs: 0,
      dropped: 0,
      droppedPct: 0,
    });
  });

  it('reports a steady 60 fps capture with no drops', () => {
    const stats = statsFromIntervals(repeat(1000 / 60, 120));
    expect(stats.frames).toBe(120);
    expect(stats.avgFps).toBeCloseTo(60, 5);
    expect(stats.p50Ms).toBeCloseTo(16.667, 2); // capped at the observed max, not the bin edge
    expect(stats.p95Ms).toBeCloseTo(16.667, 2);
    expect(stats.dropped).toBe(0);
    expect(stats.droppedPct).toBe(0);
  });

  it('finds the tail and the dropped-frame share of a janky capture', () => {
    // 90 good frames, 9 frames that missed one vsync, 1 frame that missed four.
    const intervals = [...repeat(16.6, 90), ...repeat(33.3, 9), 83.4];
    const stats = statsFromIntervals(intervals);
    expect(stats.frames).toBe(100);
    expect(stats.p50Ms).toBe(17); // upper edge of the 16.5-17.0 bin
    expect(stats.p95Ms).toBe(33.5); // upper edge of the 33.0-33.5 bin
    expect(stats.p99Ms).toBe(33.5);
    expect(stats.maxMs).toBe(83.4);
    expect(stats.dropped).toBe(9 + 4);
    expect(stats.droppedPct).toBeCloseTo((13 / 113) * 100, 6);
    expect(stats.avgFps).toBeCloseTo((100 * 1000) / (90 * 16.6 + 9 * 33.3 + 83.4), 6);
  });

  it('puts very long frames in the overflow bin and reports them as the max', () => {
    const stats = statsFromIntervals([...repeat(16.7, 98), 250, 400]);
    expect(stats.p99Ms).toBe(400); // the 99th frame (250 ms) is past MAX_BINNED_MS, so it reads as the max
    expect(stats.maxMs).toBe(400);
  });

  it('ignores non-finite and non-positive intervals', () => {
    const stats = statsFromIntervals([16.7, 0, -3, Number.NaN, Number.POSITIVE_INFINITY, 16.7]);
    expect(stats.frames).toBe(2);
  });

  it('keeps percentile error within one bin', () => {
    const intervals = Array.from({ length: 1000 }, (_, i) => 8 + (i % 200) * 0.137);
    const sorted = [...intervals].sort((a, b) => a - b);
    const exactP95 = sorted[Math.ceil(0.95 * sorted.length) - 1]!;
    const stats = statsFromIntervals(intervals);
    expect(stats.p95Ms).toBeGreaterThanOrEqual(exactP95);
    expect(stats.p95Ms - exactP95).toBeLessThanOrEqual(BIN_MS);
  });
});

describe('histogram lifecycle', () => {
  it('resets to an empty histogram in place', () => {
    const h = createFrameHistogram();
    recordFrame(h, 40, 60);
    resetFrameHistogram(h);
    expect(summarizeFrames(h)).toEqual(statsFromIntervals([]));
    expect(h.counts.every((c) => c === 0)).toBe(true);
  });
});
