import { describe, expect, it } from 'vitest';

import {
  boardToScreen,
  clampZoom,
  cullPins,
  fitCamera,
  panBy,
  pinBounds,
  sceneBounds,
  screenToBoard,
  textureBytes,
  textureLod,
  visibleRect,
  ZOOM_LIMITS,
  zoomAround,
  type PinGeometry,
} from './board';

const pin = (over: Partial<PinGeometry> = {}): PinGeometry => ({
  x: 0,
  y: 0,
  scale: 1,
  rotation: 0,
  width: 100,
  height: 50,
  ...over,
});

describe('board camera', () => {
  it('round-trips board and screen points', () => {
    const camera = { x: 40, y: -20, zoom: 2 };
    const p = { x: 13, y: 7 };
    expect(screenToBoard(camera, boardToScreen(camera, p))).toEqual(p);
    expect(boardToScreen(camera, { x: 40, y: -20 })).toEqual({ x: 0, y: 0 });
  });

  it('clamps zoom to the design-system range and survives bad input', () => {
    expect(clampZoom(10)).toBe(ZOOM_LIMITS.max);
    expect(clampZoom(0.01)).toBe(ZOOM_LIMITS.min);
    expect(clampZoom(Number.NaN)).toBe(1);
  });

  it('keeps the focal point under the fingers while zooming', () => {
    const camera = { x: 10, y: 20, zoom: 1 };
    const focal = { x: 150, y: 90 };
    const before = screenToBoard(camera, focal);
    const after = zoomAround(camera, focal, 2);
    expect(after.zoom).toBe(2);
    const moved = screenToBoard(after, focal);
    expect(moved.x).toBeCloseTo(before.x);
    expect(moved.y).toBeCloseTo(before.y);
  });

  it('pans by screen distance regardless of zoom', () => {
    expect(panBy({ x: 0, y: 0, zoom: 2 }, 100, -50)).toEqual({ x: -50, y: 25, zoom: 2 });
  });

  it('grows the visible area by a margin', () => {
    expect(visibleRect({ x: 0, y: 0, zoom: 2 }, { width: 200, height: 100 }, 0.5)).toEqual({
      x: -50,
      y: -25,
      width: 200,
      height: 100,
    });
  });
});

describe('pin bounds and culling', () => {
  it('rotates and scales the bounds', () => {
    expect(pinBounds(pin({ scale: 2 }))).toEqual({ x: -100, y: -50, width: 200, height: 100 });
    const r = pinBounds(pin({ rotation: 90 }));
    expect(r.width).toBeCloseTo(50);
    expect(r.height).toBeCloseTo(100);
  });

  it('drops pins that are entirely off screen and keeps order', () => {
    const pins = [pin({ x: 50 }), pin({ x: 5000 }), pin({ x: -40, y: 30 })];
    const shown = cullPins(pins, { x: 0, y: 0, width: 400, height: 300 });
    expect(shown).toEqual([pins[0], pins[2]]);
  });

  it('keeps a pin whose rotated corner pokes into view', () => {
    const tilted = pin({ x: -60, y: 0, rotation: 45, width: 100, height: 100 });
    expect(cullPins([tilted], { x: 0, y: -10, width: 100, height: 20 })).toHaveLength(1);
  });

  it('frames all pins with zoom to fit', () => {
    const area = sceneBounds([pin({ x: 0 }), pin({ x: 400 })])!;
    const camera = fitCamera(area, { width: 450, height: 800 });
    const tl = boardToScreen(camera, { x: area.x, y: area.y });
    expect(tl.x).toBeCloseTo(0);
    expect(sceneBounds([])).toBeNull();
  });
});

describe('texture level of detail', () => {
  it('uses the small texture under 30% of the screen and sharper levels when zoomed', () => {
    expect(textureLod(300, 2400)).toBe(512);
    expect(textureLod(900, 2400)).toBe(1024);
    expect(textureLod(1500, 2400)).toBe(2048);
    expect(textureLod(5000, 2400)).toBe(2048);
    expect(textureLod(0, 2400)).toBe(512);
  });

  it('counts RGBA bytes', () => {
    expect(textureBytes(512, 512)).toBe(1_048_576);
  });
});
