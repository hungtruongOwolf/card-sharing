/**
 * Draws whole cards (cut style + finish) with real Skia (CanvasKit WASM) offscreen and probes pixels.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import type { CutScene } from '../../cut-styles';
import { downscalePhoto, drawCard, previewSize, subjectFor } from '../cardScene';
import { drawFinish } from '../drawFinish';
import { styleChoice, type CardDraft } from '../editorState';
import { syntheticMask } from '../maskSource';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 96;
const PHOTO = [51, 102, 204, 255];

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

function solidImage(width: number, height: number): SkImage {
  const surface = Skia.Surface.Make(width, height)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  return surface.makeImageSnapshot();
}

function scene(): CutScene {
  return {
    width: SIZE,
    height: SIZE,
    photo: solidImage(SIZE, SIZE),
    subject: subjectFor(syntheticMask(SIZE, SIZE), SIZE),
  };
}

function render(draft: CardDraft): (x: number, y: number) => number[] {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  drawCard(surface.getCanvas(), registry, draft, scene());
  surface.flush();
  const pixels = surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
  return (x, y) => Array.from(pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4));
}

const draft = (cut: string, finish: string | null = null, wholePhoto = false): CardDraft => ({
  cut: styleChoice(entry(cut)),
  finish: finish ? styleChoice(entry(finish)) : null,
  wholePhoto,
  caption: '',
});

describe('drawCard', () => {
  it('cuts out the subject, transparent around it', () => {
    const at = render(draft('cut.raw'));
    expect(at(SIZE / 2, Math.round(SIZE * 0.6))).toEqual(PHOTO);
    expect(at(2, 2)[3]).toBe(0);
  });

  it('draws the whole photo for a whole-photo draft even when the scene has a subject', () => {
    const at = render(draft('cut.none', null, true));
    expect(at(SIZE / 2, SIZE / 2)).toEqual(PHOTO);
    expect(at(SIZE / 2, 1)).toEqual(PHOTO);
  });

  it('throws when a mask-based style meets a whole-photo draft (the picker never offers it)', () => {
    expect(() => render(draft('cut.raw', null, true))).toThrow(/needs a subject mask/);
  });

  it('lays the holo finish over the card pixels only', () => {
    const plain = render(draft('cut.raw'));
    const holo = render(draft('cut.raw', 'fx.holo'));
    const inside: [number, number] = [SIZE / 2, Math.round(SIZE * 0.6)];
    expect(holo(...inside)).not.toEqual(plain(...inside));
    expect(holo(2, 2)[3]).toBe(0);
  });

  it.each(['fx.matte', 'fx.glossy'])('lays %s over the card pixels only', (finish) => {
    const plain = render(draft('cut.raw'));
    const finished = render(draft('cut.raw', finish));
    const inside: [number, number] = [SIZE / 2, Math.round(SIZE * 0.6)];
    expect(finished(...inside)).not.toEqual(plain(...inside));
    expect(finished(...inside)[3]).toBe(255);
    expect(finished(2, 2)[3]).toBe(0);
  });

  it('renders every finish byte-stable, so baked cards can be golden-tested', () => {
    for (const finish of ['fx.matte', 'fx.glossy', 'fx.holo']) {
      const first = render(draft('cut.none', finish));
      const second = render(draft('cut.none', finish));
      for (const [x, y] of [
        [10, 10],
        [48, 48],
        [80, 20],
      ] as const) {
        expect(second(x, y)).toEqual(first(x, y));
      }
    }
  });

  it('keeps the photo readable under the default matte wash', () => {
    const at = render(draft('cut.none', 'fx.matte'));
    const [r, g, b] = at(SIZE / 2, SIZE / 2);
    // Still clearly the blue photo: blue dominates even at the default intensity.
    expect(b! - r!).toBeGreaterThan(60);
    expect(g).toBeLessThan(200);
  });

  it('draws nothing for an id the registry no longer has', () => {
    const at = render({ ...draft('cut.none'), cut: { id: 'cut.gone', version: 1, params: {} } });
    expect(at(SIZE / 2, SIZE / 2)[3]).toBe(0);
  });

  it('rejects finishes without a renderer', () => {
    const surface = Skia.Surface.Make(4, 4)!;
    expect(() => drawFinish(surface.getCanvas(), entry('cut.none'), {}, 4, 4)).toThrow(
      /no finish renderer/,
    );
  });
});

describe('preview geometry', () => {
  it('fits the photo aspect ratio into the preview box', () => {
    expect(previewSize({ width: 3000, height: 4000 }, 360, 440)).toEqual({
      width: 330,
      height: 440,
    });
    expect(previewSize({ width: 4000, height: 3000 }, 360, 440)).toEqual({
      width: 360,
      height: 270,
    });
  });

  it('downscales large photos once and keeps small ones', () => {
    const large = solidImage(400, 200);
    const small = downscalePhoto(large, 100);
    expect([small.width(), small.height()]).toEqual([100, 50]);
    expect(downscalePhoto(small, 100)).toBe(small);
  });
});
