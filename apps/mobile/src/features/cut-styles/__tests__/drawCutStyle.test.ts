/**
 * Renders cut styles with real Skia (CanvasKit WASM) offscreen and checks pixels.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { createMask, maskToContours } from '@card-sharing/canvas-engine';
import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import { SCHEMES } from '../../../test/render';
import { buildTheme } from '../../../ui';
import { borderWidthPx, cornerRadiusPx } from '../cutGeometry';
import { drawCutStyle, hasCutRenderer, type CutScene } from '../drawCutStyle';
import { subjectPath } from '../subjectPath';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 128;
const CENTER = 64;
const RADIUS = 40;
const PHOTO = [51, 102, 204, 255];

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

let cachedScene: CutScene | undefined;

/** Built once: tracing the contour and baking the photo per render made the suite slow under a full run. */
function discScene(): CutScene {
  cachedScene ??= buildDiscScene();
  return cachedScene;
}

function buildDiscScene(): CutScene {
  const data = new Uint8Array(SIZE * SIZE);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      if ((x + 0.5 - CENTER) ** 2 + (y + 0.5 - CENTER) ** 2 <= RADIUS ** 2)
        data[y * SIZE + x] = 255;
    }
  }
  const subject = subjectPath(maskToContours(createMask(SIZE, SIZE, data)), 1)!;
  const photoSurface = Skia.Surface.Make(SIZE, SIZE)!;
  photoSurface.getCanvas().clear(Skia.Color('#3366CC'));
  photoSurface.flush();
  return {
    width: SIZE,
    height: SIZE,
    photo: photoSurface.makeImageSnapshot(),
    subject,
  };
}

/** The disc scene without its subject: a whole-photo card (no on-device cutout, EM 2026-11-05). */
const wholePhotoScene = (): CutScene => ({ ...discScene(), subject: null });

function render(
  id: string,
  params: unknown,
  background?: string,
  scene: CutScene = discScene(),
): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  const canvas = surface.getCanvas();
  if (background) canvas.clear(Skia.Color(background));
  drawCutStyle(canvas, entry(id), params, scene);
  surface.flush();
  return pixelsOf(surface.makeImageSnapshot());
}

function pixelsOf(image: SkImage): Uint8Array {
  const info = {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  };
  return image.readPixels(0, 0, info) as Uint8Array;
}

const at = (pixels: Uint8Array, x: number, y: number) =>
  Array.from(pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4));

/** Coarse 8x8 signature (hex per sample) for light/dark goldens: stable, readable in a snapshot diff. */
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

describe('cut style renderers', () => {
  it('cover every cut style in the registry', () => {
    expect(registry.byKind('cutStyle').filter((e) => !hasCutRenderer(e))).toEqual([]);
  });

  it('cut.none: photo with rounded corners', () => {
    const pixels = render('cut.none', { cornerRadiusPct: 12 });
    expect(at(pixels, 0, 0)[3]).toBe(0);
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    expect(at(pixels, CENTER, 0)).toEqual(PHOTO);
  });

  it('cut.raw: subject only, transparent outside', () => {
    const pixels = render('cut.raw', {});
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    expect(at(pixels, CENTER + RADIUS + 3, CENTER)[3]).toBe(0);
    expect(at(pixels, 2, 2)[3]).toBe(0);
  });

  it('cut.diecut: border colour just outside the subject, nothing beyond it', () => {
    const scene = discScene();
    const border = borderWidthPx(scene.subject!.bounds, 6);
    expect(border).toBeGreaterThan(6);
    const pixels = render('cut.diecut', { borderWidthPct: 6 });
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    // Edge cleanup erodes 1 px, so the outline sits near RADIUS - 1.
    const inBorder = Math.round(CENTER + RADIUS - 1 + border / 2);
    expect(at(pixels, inBorder, CENTER)).toEqual([255, 255, 255, 255]);
    expect(at(pixels, Math.ceil(CENTER + RADIUS + border + 2), CENTER)[3]).toBe(0);
  });

  it('cut.diecut: border width clamps to the 0.5-6 % range', () => {
    expect(render('cut.diecut', { borderWidthPct: 40 })).toEqual(
      render('cut.diecut', { borderWidthPct: 6 }),
    );
    expect(render('cut.diecut', { borderWidthPct: 0 })).toEqual(
      render('cut.diecut', { borderWidthPct: 0.5 }),
    );
    expect(render('cut.diecut', { color: '#FF0000' })).not.toEqual(render('cut.diecut', {}));
  });

  it('refuses mask-based styles on a whole-photo card', () => {
    expect(() => render('cut.raw', {}, undefined, wholePhotoScene())).toThrow(/needs a subject/);
    expect(() => render('cut.diecut', {}, undefined, wholePhotoScene())).toThrow(/needs a subject/);
  });

  it.each(SCHEMES)('matches the golden signatures on the %s canvas', (scheme) => {
    const background = buildTheme(scheme, false).color.bg.canvas;
    const goldens = Object.fromEntries(
      registry.byKind('cutStyle').map((e) => [e.id, signature(render(e.id, {}, background))]),
    );
    expect(goldens).toMatchSnapshot();
  });

  it.each(SCHEMES)('matches the whole-photo golden signatures on the %s canvas', (scheme) => {
    const background = buildTheme(scheme, false).color.bg.canvas;
    const maskFree = registry
      .byKind('cutStyle')
      .filter((e) => e.platforms.ios.requires.length === 0);
    const goldens = Object.fromEntries(
      maskFree.map((e) => [e.id, signature(render(e.id, {}, background, wholePhotoScene()))]),
    );
    expect(goldens).toMatchSnapshot();
  });
});

describe('shape cuts (whole photo, T-63)', () => {
  const shape = (id: string, params: unknown = {}) =>
    render(id, params, undefined, wholePhotoScene());
  const WHITE = [255, 255, 255, 255];

  it('cut.shape.circle: photo inside the circle, transparent corners, border at the rim', () => {
    const pixels = shape('cut.shape.circle', { borderWidthPct: 6 });
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    expect(at(pixels, 6, 6)[3]).toBe(0);
    // Square side 0.96 * 128 ≈ 123 px: radius ≈ 61, border ≈ 7 px.
    expect(at(pixels, CENTER + 58, CENTER)).toEqual(WHITE);
    expect(at(pixels, CENTER + 50, CENTER)).toEqual(PHOTO);
  });

  it('cut.shape.heart: photo in the lobes, transparent in the top notch and the corners', () => {
    const pixels = shape('cut.shape.heart', { borderWidthPct: 0 });
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    expect(at(pixels, CENTER - 25, 30)).toEqual(PHOTO);
    expect(at(pixels, CENTER, 14)[3]).toBe(0);
    expect(at(pixels, 4, 124)[3]).toBe(0);
  });

  it('cut.shape.rounded-square: rounded corners, corner radius clamps to 4-30 %', () => {
    const pixels = shape('cut.shape.rounded-square', { borderWidthPct: 0, cornerRadiusPct: 30 });
    expect(at(pixels, 4, 4)[3]).toBe(0);
    expect(at(pixels, CENTER, 4)).toEqual(PHOTO);
    expect(shape('cut.shape.rounded-square', { cornerRadiusPct: 90 })).toEqual(
      shape('cut.shape.rounded-square', { cornerRadiusPct: 30 }),
    );
  });

  it('use the photo, not the subject, when a subject exists', () => {
    expect(render('cut.shape.circle', {})).toEqual(shape('cut.shape.circle'));
  });
});

describe('cut.torn (T-63)', () => {
  it('is deterministic for a seed and changes with it', () => {
    expect(render('cut.torn', { seed: 42 })).toEqual(render('cut.torn', { seed: 42 }));
    expect(render('cut.torn', { seed: 42 })).not.toEqual(render('cut.torn', { seed: 43 }));
    const whole = (seed: number) => render('cut.torn', { seed }, undefined, wholePhotoScene());
    expect(whole(5)).toEqual(whole(5));
    expect(whole(5)).not.toEqual(whole(6));
  });

  it('puts the subject on a paper scrap that follows its contour', () => {
    const pixels = render('cut.torn', { roughness: 0.2 });
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    // Paper margin is 4.5 % of 128 ≈ 5.8 px beyond the outline (near RADIUS - 1).
    expect(at(pixels, CENTER + RADIUS + 3, CENTER)).toEqual([255, 253, 247, 255]);
    expect(at(pixels, CENTER + RADIUS + 14, CENTER)[3]).toBe(0);
    expect(at(pixels, 2, 2)[3]).toBe(0);
  });

  it('tears the whole photo out on a paper rim without a subject', () => {
    const pixels = render(
      'cut.torn',
      { roughness: 0.2, paper: 'kraft' },
      undefined,
      wholePhotoScene(),
    );
    expect(at(pixels, CENTER, CENTER)).toEqual(PHOTO);
    expect(at(pixels, 4, CENTER)).toEqual([201, 162, 126, 255]);
    expect(at(pixels, 0, 0)[3]).toBe(0);
  });

  it('draws grid lines only on the grid paper', () => {
    expect(render('cut.torn', { paper: 'grid' })).not.toEqual(
      render('cut.torn', { paper: 'white' }),
    );
  });
});

describe('cut geometry', () => {
  it('sizes borders by the subject diagonal and corners by the short side', () => {
    expect(borderWidthPx({ x: 0, y: 0, width: 30, height: 40 }, 6)).toBeCloseTo(3);
    expect(cornerRadiusPx(200, 100, 4)).toBe(4);
  });
});
