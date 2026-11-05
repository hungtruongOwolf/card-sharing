import { describe, expect, it } from 'vitest';

import { disc, maskFrom, ring, seeded, union, withNoise } from './fixtures';
import {
  chaikinRing,
  maskToContours,
  signedArea,
  simplifyRing,
  toPathCommands,
  toSvgPath,
} from './index';
import { createMask, edgeCleanup, erode } from './mask';
import { traceContours, type Point, type Ring } from './marchingSquares';

const areaOf = (points: Ring) => Math.abs(signedArea(points));
const centroid = (points: Ring): Point => ({
  x: points.reduce((s, p) => s + p.x, 0) / points.length,
  y: points.reduce((s, p) => s + p.y, 0) / points.length,
});

describe('traceContours', () => {
  it('returns nothing for empty and full masks are closed by the padding', () => {
    expect(traceContours(createMask(8, 8))).toEqual([]);
    const full = traceContours(createMask(4, 4, new Uint8Array(16).fill(255)));
    expect(full).toHaveLength(1);
    // Corner cells chamfer the square: 16 px minus four 0.125 px triangles.
    expect(areaOf(full[0]!)).toBeCloseTo(15.5, 1);
  });

  it('traces a disc with the right area, centre and orientation', () => {
    const [outline, ...rest] = traceContours(maskFrom(64, 64, disc(32, 32, 20)));
    expect(rest).toEqual([]);
    expect(signedArea(outline!)).toBeLessThan(0);
    expect(areaOf(outline!) / (Math.PI * 400)).toBeCloseTo(1, 1);
    const c = centroid(outline!);
    expect(c.x).toBeCloseTo(32, 0);
    expect(c.y).toBeCloseTo(32, 0);
  });

  it('gives holes the opposite orientation', () => {
    const rings = traceContours(maskFrom(64, 64, ring(32, 32, 10, 24)));
    expect(rings.map((r) => Math.sign(signedArea(r))).sort()).toEqual([-1, 1]);
  });

  it('separates islands and resolves saddles consistently', () => {
    expect(traceContours(maskFrom(64, 32, union(disc(14, 16, 8), disc(48, 16, 8))))).toHaveLength(
      2,
    );
    // Diagonal pixels: a saddle cell. Disconnected at threshold 128 because the centre average is 127.5.
    const diagonal = createMask(2, 2, Uint8Array.from([255, 0, 0, 255]));
    expect(traceContours(diagonal)).toHaveLength(2);
    expect(traceContours(diagonal, 100)).toHaveLength(1);
  });

  it('interpolates crossings on soft edges', () => {
    // Between the 192 sample (centre x = 1.5) and the 0 sample (x = 2.5), 128 sits a third of the way.
    const soft = createMask(3, 1, Uint8Array.from([255, 192, 0]));
    const [outline] = traceContours(soft);
    expect(Math.max(...outline!.map((p) => p.x))).toBeCloseTo(1.5 + 1 / 3, 5);
  });
});

describe('simplifyRing and chaikinRing', () => {
  const circle: Ring = Array.from({ length: 360 }, (_, i) => ({
    x: 50 + 30 * Math.cos((i * Math.PI) / 180),
    y: 50 + 30 * Math.sin((i * Math.PI) / 180),
  }));

  it('keeps a subset within epsilon of the input', () => {
    const simplified = simplifyRing(circle, 0.5);
    expect(simplified.length).toBeLessThan(60);
    expect(simplified.every((p) => circle.includes(p))).toBe(true);
    expect(areaOf(simplified) / areaOf(circle)).toBeGreaterThan(0.98);
  });

  it('smooths corners: doubles points per iteration and stays inside the bounds', () => {
    const square: Ring = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 10 },
      { x: 0, y: 10 },
    ];
    const smooth = chaikinRing(square, 3);
    expect(smooth).toHaveLength(32);
    expect(smooth.every((p) => p.x >= 0 && p.x <= 10 && p.y >= 0 && p.y <= 10)).toBe(true);
    expect(areaOf(smooth)).toBeLessThan(100);
  });
});

describe('maskToContours', () => {
  it('produces one smooth outer contour for a disc', () => {
    const { contours, bounds } = maskToContours(maskFrom(128, 128, disc(64, 64, 40)));
    expect(contours).toHaveLength(1);
    expect(contours[0]!.hole).toBe(false);
    // Edge cleanup erodes 1 px, so the radius shrinks by about one pixel.
    expect(contours[0]!.area / (Math.PI * 39 * 39)).toBeCloseTo(1, 1);
    expect(bounds!.x).toBeGreaterThan(24);
    expect(bounds!.x).toBeLessThan(26.5);
    expect(bounds!.width).toBeGreaterThan(76);
    expect(bounds!.width).toBeLessThan(79);
  });

  it('keeps holes and islands, flagged', () => {
    const shape = union(ring(40, 40, 12, 30), disc(100, 40, 15));
    const { contours } = maskToContours(maskFrom(128, 80, shape));
    expect(contours.map((c) => c.hole).sort()).toEqual([false, false, true]);
  });

  it('drops noise specks and pinholes (property, 20 seeds)', () => {
    for (let seed = 1; seed <= 20; seed++) {
      const random = seeded(seed);
      const r = 40 + random() * 60;
      const noisy = withNoise(maskFrom(256, 256, disc(128, 128, r)), 0.01, random);
      const { contours } = maskToContours(noisy);
      expect(contours.filter((c) => !c.hole)).toHaveLength(1);
      expect(contours.filter((c) => c.hole)).toHaveLength(0);
      expect(contours[0]!.area / (Math.PI * (r - 1) ** 2)).toBeGreaterThan(0.85);
      expect(contours[0]!.area / (Math.PI * (r - 1) ** 2)).toBeLessThan(1.1);
    }
  });

  it('returns no contours and null bounds for an empty mask', () => {
    expect(maskToContours(createMask(16, 16))).toEqual({ contours: [], bounds: null });
  });

  it('matches the golden path for a small fixture', () => {
    const { contours } = maskToContours(maskFrom(24, 24, disc(12, 12, 8)), { smoothing: 1 });
    expect(toSvgPath(toPathCommands(contours.map((c) => c.points)))).toMatchSnapshot();
  });

  it('bakes a 1024² mask within the CI budget', () => {
    const mask = maskFrom(1024, 1024, union(ring(400, 500, 80, 300), disc(820, 300, 120)));
    maskToContours(mask); // warm up the JIT
    const start = Date.now();
    const { contours } = maskToContours(mask);
    const elapsed = Date.now() - start;
    expect(contours).toHaveLength(3);
    // Device target is <= 80 ms on a mid-range phone (T-61); CI/Node budget is looser and catches regressions.
    expect(elapsed).toBeLessThan(1500);
  });
});

describe('edgeCleanup', () => {
  it('erodes one pixel from every side, including the image border', () => {
    const full = createMask(5, 5, new Uint8Array(25).fill(255));
    const eroded = erode(full);
    expect([...eroded.data].filter((v) => v === 255)).toHaveLength(9);
  });

  it('feathers to a soft edge without changing a solid interior', () => {
    const cleaned = edgeCleanup(maskFrom(32, 32, disc(16, 16, 10)));
    expect(cleaned.data[16 * 32 + 16]).toBe(255);
    expect(cleaned.data[0]).toBe(0);
    expect([...cleaned.data].some((v) => v > 0 && v < 255)).toBe(true);
  });

  it('validates mask sizes', () => {
    expect(() => createMask(0, 4)).toThrow(RangeError);
    expect(() => createMask(2, 2, new Uint8Array(3))).toThrow(RangeError);
  });
});

describe('toPathCommands', () => {
  it('emits move, lines and close per ring, scaled', () => {
    const square: Ring = [
      { x: 0, y: 0 },
      { x: 1, y: 0 },
      { x: 1, y: 1 },
    ];
    expect(toPathCommands([square], 2)).toEqual([['M', 0, 0], ['L', 2, 0], ['L', 2, 2], ['Z']]);
    expect(toSvgPath(toPathCommands([square], 1 / 3))).toBe('M0 0 L0.33 0 L0.33 0.33 Z');
  });
});
