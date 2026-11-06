/**
 * Fire breath (T-122) through the whole card pipeline with real Skia (CanvasKit WASM), on the anchor fallback.
 * @jest-environment <rootDir>/jest.skia-environment.js
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
  effects: [styleChoice(registry.get('fx.fire-breath')!, params)],
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

const fire = { x: 0.4, y: 0.5, angle: 0, length: 0.5, spread: 20, seed: 4 };

describe('fire breath', () => {
  it('is draggable and starts at the mouth from the fallback chain (no native landmarks)', () => {
    const entry = registry.get('fx.fire-breath')!;
    expect(hasEffectRenderer(entry)).toBe(true);
    expect(placementTargets(entry)).toEqual(['body']);
    expect(entry.platforms.ios).toMatchObject({
      requires: ['faceLandmarks'],
      fallback: 'userPoints',
    });
    const data = new Uint8Array(100 * 100);
    for (let y = 20; y < 100; y++) for (let x = 30; x < 70; x++) data[y * 100 + x] = 255;
    const anchors = resolveAnchors(null, createMask(100, 100, data));
    const params = placeEffect(entry, anchors, () => 0);
    expect([params.x, params.y]).toEqual([anchors.mouth.x, anchors.mouth.y]);
    expect(params.seed).toBe(1);
  });

  it('shoots a hot cone from the mouth along the angle, nothing behind it', () => {
    const pixels = render(card(fire));
    const near = at(pixels, 60, 64);
    expect(near[0]!).toBeGreaterThan(200);
    expect(near[1]!).toBeGreaterThan(PHOTO[1]!);
    expect(at(pixels, 20, 64)).toEqual(PHOTO);
    expect(at(pixels, 64, 10)).toEqual(PHOTO);
    const left = render(card({ ...fire, x: 0.6, angle: 180 }));
    expect(at(left, 50, 64)[0]!).toBeGreaterThan(200);
    expect(at(left, 110, 64)).toEqual(PHOTO);
  });

  it('keeps the particle budget and a lighter default', () => {
    const entry = registry.get('fx.fire-breath')!;
    const puffs = entry.params.puffs!;
    expect(puffs.type === 'integer' && [puffs.max, puffs.default]).toEqual([300, 40]);
    expect(render(card({ ...fire, puffs: 300 }))).not.toEqual(render(card(fire)));
  });

  it('flickers with the clock in a seamless loop, a still at time 0', () => {
    const still = render(card(fire));
    expect(render(card(fire), { time: 0.5 })).not.toEqual(still);
    expect(render(card(fire), { time: 2 })).toEqual(still);
    expect(registry.get('fx.fire-breath')?.cost.liveAllowed).toBe(false);
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches the default and a sideways flame', () => {
    expect({
      default: signature(render(card({ x: 0.5, y: 0.5, seed: 2 }), { background })),
      sideways: signature(render(card(fire), { background })),
    }).toMatchSnapshot();
  });
});
