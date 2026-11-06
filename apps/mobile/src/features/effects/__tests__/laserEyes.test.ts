/**
 * Laser eyes (T-121) through the whole card pipeline with real Skia (CanvasKit WASM), on the anchor fallback.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia } from '@shopify/react-native-skia';

import { createMask } from '@card-sharing/canvas-engine';
import { registry, type ParamValue } from '@card-sharing/effects-registry';

import { SCHEMES } from '../../../test/render';
import { buildTheme } from '../../../ui';
import type { CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { placeEffect, resolveAnchors } from '../anchors';
import { hasEffectRenderer } from '../compose';
import { placementTargets } from '../placement';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 128;
const PHOTO = [51, 102, 204, 255];

function scene(color = '#3366CC'): CutScene {
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  photo.getCanvas().clear(Skia.Color(color));
  photo.flush();
  return { width: SIZE, height: SIZE, photo: photo.makeImageSnapshot(), subject: null };
}

const card = (params: Record<string, ParamValue> = {}): CardDraft => ({
  cut: styleChoice(registry.get('cut.none')!),
  finish: null,
  effects: [styleChoice(registry.get('fx.laser-eyes')!, params)],
  wholePhoto: true,
  caption: '',
});

function render(
  draft: CardDraft,
  options: { background?: string; time?: number; photo?: string } = {},
): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  if (options.background) surface.getCanvas().clear(Skia.Color(options.background));
  drawCard(surface.getCanvas(), registry, draft, scene(options.photo), { time: options.time });
  surface.flush();
  return surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
}

const at = (pixels: Uint8Array, x: number, y: number) =>
  Array.from(pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4));

function signature(pixels: Uint8Array): string[] {
  const rows: string[] = [];
  for (let y = SIZE / 16; y < SIZE; y += SIZE / 8) {
    const row: string[] = [];
    for (let x = SIZE / 16; x < SIZE; x += SIZE / 8) {
      row.push(
        at(pixels, x, y)
          .map((v) => v.toString(16).padStart(2, '0'))
          .join(''),
      );
    }
    rows.push(row.join(' '));
  }
  return rows;
}

const eyes = { x: 0.5, y: 0.3, eyeGap: 0.25, angle: 0, beamLength: 1, flare: false };

describe('laser eyes', () => {
  it('is draggable and placed on the eyes by the fallback chain (no native landmarks)', () => {
    const entry = registry.get('fx.laser-eyes')!;
    expect(hasEffectRenderer(entry)).toBe(true);
    expect(placementTargets(entry)).toEqual(['body']);
    expect(entry.platforms.android).toMatchObject({
      requires: ['faceLandmarks'],
      fallback: 'userPoints',
    });
    const data = new Uint8Array(100 * 100);
    for (let y = 20; y < 100; y++) for (let x = 30; x < 70; x++) data[y * 100 + x] = 255;
    const anchors = resolveAnchors(null, createMask(100, 100, data));
    const params = placeEffect(entry, anchors);
    expect(params.x).toBeCloseTo(0.5);
    expect(params.y).toBeCloseTo(anchors.eyes[0].y);
    expect(params.eyeGap).toBeCloseTo(anchors.eyes[1].x - anchors.eyes[0].x);
  });

  it('puts a hot core on each eye and beams along the angle', () => {
    const pixels = render(card(eyes));
    const left = at(pixels, 48, 38);
    const right = at(pixels, 80, 38);
    for (const core of [left, right]) {
      expect(core[0]).toBeGreaterThan(200);
      expect(core[3]).toBe(255);
    }
    // Along the beam (to the right of the right eye) red rises; above the eyes the photo is untouched.
    expect(at(pixels, 110, 38)[0]!).toBeGreaterThan(PHOTO[0]! + 40);
    expect(at(pixels, 64, 5)).toEqual(PHOTO);
  });

  it('glows on a dark photo and on a light one (additive)', () => {
    const dark = render(card(eyes), { photo: '#101010' });
    expect(at(dark, 110, 38)[0]!).toBeGreaterThan(80);
    const light = render(card(eyes), { photo: '#DDDDDD' });
    expect(at(light, 48, 38)).toEqual([255, 255, 255, 255]);
  });

  it('pulses with the clock in a seamless loop, a still at time 0', () => {
    const still = render(card(eyes));
    expect(render(card(eyes), { time: 0.5 })).not.toEqual(still);
    expect(render(card(eyes), { time: 2 })).toEqual(still);
    expect(registry.get('fx.laser-eyes')?.cost.liveAllowed).toBe(false);
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches the default and a flare-free beam', () => {
    expect({
      default: signature(render(card({ x: 0.5, y: 0.35 }), { background })),
      straight: signature(render(card(eyes), { background })),
    }).toMatchSnapshot();
  });
});
