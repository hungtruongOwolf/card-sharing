import { createMask } from '@card-sharing/canvas-engine';
import { registry } from '@card-sharing/effects-registry';

import {
  anchorPoint,
  DEFAULT_ANCHORS,
  maskAnchors,
  placeEffect,
  resolveAnchors,
  type NativeAnchors,
} from '../anchors';
import { movePlacement, placementTargets } from '../placement';

/** A 100 x 100 mask with a subject block x 30..70, y 20..100 (head band = y 20..36). */
function blockMask() {
  const data = new Uint8Array(100 * 100);
  for (let y = 20; y < 100; y++) for (let x = 30; x < 70; x++) data[y * 100 + x] = 255;
  return createMask(100, 100, data);
}

const native: NativeAnchors = {
  head: { x: 0.2, y: 0.2 },
  top: { x: 0.2, y: 0.1 },
  eyes: [
    { x: 0.18, y: 0.19 },
    { x: 0.22, y: 0.19 },
  ],
  mouth: { x: 0.2, y: 0.26 },
  headWidth: 0.1,
};

describe('anchor port (fallback chain)', () => {
  it('reads head, crown, eyes and mouth from the top band of the subject mask', () => {
    const anchors = maskAnchors(blockMask())!;
    expect(anchors.source).toBe('mask');
    expect(anchors.head.x).toBeCloseTo(0.5);
    expect(anchors.top).toEqual({ x: 0.5, y: 0.2 });
    expect(anchors.head.y).toBeGreaterThan(0.2);
    expect(anchors.mouth.y).toBeGreaterThan(anchors.eyes[0].y);
    expect(anchors.eyes[0].x).toBeLessThan(anchors.eyes[1].x);
    expect(anchors.headWidth).toBeCloseTo(0.4);
    expect(maskAnchors(createMask(10, 10, new Uint8Array(100)))).toBeNull();
  });

  it('prefers native landmarks, then the mask, then the default guess', () => {
    expect(resolveAnchors(native, blockMask()).source).toBe('native');
    expect(resolveAnchors(null, blockMask()).source).toBe('mask');
    expect(resolveAnchors(null, null)).toBe(DEFAULT_ANCHORS);
  });

  it('auto-places a speech bubble tail on the mouth and the bubble beside it', () => {
    const speech = registry.get('overlay.speech')!;
    const anchors = resolveAnchors(native, null);
    expect(anchorPoint(speech, anchors)).toEqual(native.mouth);
    const params = placeEffect(speech, anchors);
    expect([params.tailX, params.tailY]).toEqual([0.2, 0.26]);
    expect(params.x).toBeCloseTo(0.48);
    expect(params.y).toBeCloseTo(0.1);
  });

  it('gives tape a fresh seed and leaves unanchored placement at its defaults', () => {
    const washi = registry.get('overlay.washi')!;
    expect(placeEffect(washi, DEFAULT_ANCHORS, () => 0.5)).toEqual({ seed: 5000 });
    expect(anchorPoint(washi, DEFAULT_ANCHORS)).toBeNull();
  });
});

describe('placement', () => {
  it('lists the drag targets from hidden point params', () => {
    expect(placementTargets(registry.get('overlay.speech')!)).toEqual(['body', 'tail']);
    expect(placementTargets(registry.get('overlay.washi')!)).toEqual(['body']);
    expect(placementTargets(registry.get('frame.polaroid')!)).toEqual([]);
  });

  it('moves a target from where the drag started and keeps it on the card', () => {
    const start = { x: 0.5, y: 0.5, tailX: 0.1, tailY: 0.1 };
    expect(movePlacement(start, 'body', start, 0.2, -0.1)).toEqual({ ...start, x: 0.7, y: 0.4 });
    expect(movePlacement(start, 'tail', start, -0.5, 2)).toEqual({ ...start, tailX: 0, tailY: 1 });
  });
});
