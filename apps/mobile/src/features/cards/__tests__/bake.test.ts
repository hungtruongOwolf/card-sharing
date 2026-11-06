/**
 * Bakes cards offscreen with real Skia (CanvasKit WASM) and decodes the PNGs back.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import { styleChoice, type CardDraft } from '../../editor/editorState';
import { syntheticMask } from '../../editor/maskSource';
import { BAKE_LONG_SIDE, BakeError, bakeCard, bakeSize, encodeMask, usesMask } from '../bake';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const LONG_SIDE = 96;

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

function photo(width = 120, height = 160): SkImage {
  const surface = Skia.Surface.Make(width, height)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  return surface.makeImageSnapshot();
}

const draft = (cut: string, finish: string | null = null, wholePhoto = false): CardDraft => ({
  cut: styleChoice(entry(cut)),
  finish: finish ? styleChoice(entry(finish)) : null,
  wholePhoto,
  caption: '',
});

function decode(png: Uint8Array) {
  const image = Skia.Image.MakeImageFromEncoded(Skia.Data.fromBytes(png))!;
  const width = image.width();
  const pixels = image.readPixels(0, 0, {
    width,
    height: image.height(),
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
  return {
    width,
    height: image.height(),
    at: (x: number, y: number) =>
      Array.from(pixels.slice((y * width + x) * 4, (y * width + x) * 4 + 4)),
  };
}

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47];

describe('bakeSize', () => {
  it('puts the long side on the longer photo side', () => {
    expect(bakeSize({ width: 3000, height: 4000 }, BAKE_LONG_SIDE.send)).toEqual({
      width: 810,
      height: 1080,
    });
    expect(bakeSize({ width: 4000, height: 3000 }, BAKE_LONG_SIDE.export)).toEqual({
      width: 2048,
      height: 1536,
    });
  });
});

describe('bakeCard', () => {
  const mask = syntheticMask(60, 80);

  it('renders a sticker as a PNG with alpha, plus the mask it was cut from', () => {
    const baked = bakeCard({
      registry,
      draft: draft('cut.raw', 'fx.glossy'),
      photo: photo(),
      mask,
      purpose: 'send',
      longSide: LONG_SIDE,
    });
    expect(Array.from(baked.render.png.slice(0, 4))).toEqual(PNG_MAGIC);
    const render = decode(baked.render.png);
    expect([render.width, render.height]).toEqual([72, 96]);
    expect(render.at(1, 1)[3]).toBe(0);
    expect(render.at(36, 58)[3]).toBe(255);
    expect(baked.mask).not.toBeNull();
    const decodedMask = decode(baked.mask!.png);
    expect([decodedMask.width, decodedMask.height]).toEqual([60, 80]);
    expect(decodedMask.at(30, 48).slice(0, 3)).toEqual([255, 255, 255]);
    expect(decodedMask.at(1, 1).slice(0, 3)).toEqual([0, 0, 0]);
  });

  it('is byte-stable: the same draft bakes to the same bytes', () => {
    for (const finish of [null, 'fx.matte', 'fx.glossy', 'fx.holo']) {
      const bake = () =>
        bakeCard({
          registry,
          draft: draft('cut.diecut', finish),
          photo: photo(),
          mask,
          purpose: 'send',
          longSide: LONG_SIDE,
        });
      expect(bake().render.png).toEqual(bake().render.png);
    }
  });

  it('sends no mask for whole-photo cards or styles that do not read it', () => {
    const whole = bakeCard({
      registry,
      draft: draft('cut.none', null, true),
      photo: photo(),
      mask,
      purpose: 'send',
      longSide: LONG_SIDE,
    });
    expect(whole.mask).toBeNull();
    expect(usesMask(registry, draft('cut.shape.circle'))).toBe(false);
    expect(usesMask(registry, draft('cut.diecut'))).toBe(true);
    expect(usesMask(registry, draft('cut.diecut', null, true))).toBe(false);
  });

  it('fails loudly instead of sending an empty card', () => {
    const base = { registry, photo: photo(), mask, purpose: 'send' as const, longSide: 8 };
    expect(() =>
      bakeCard({
        ...base,
        draft: { ...draft('cut.none'), cut: { id: 'cut.gone', version: 1, params: {} } },
      }),
    ).toThrow(BakeError);
    expect(() => bakeCard({ ...base, draft: draft('cut.none'), makeSurface: () => null })).toThrow(
      /no surface/,
    );
  });

  it('encodes masks at their own resolution', () => {
    const encoded = encodeMask(syntheticMask(16, 8));
    expect([encoded.width, encoded.height]).toEqual([16, 8]);
    expect(Array.from(encoded.png.slice(0, 4))).toEqual(PNG_MAGIC);
  });
});
