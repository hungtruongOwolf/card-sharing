/**
 * Sparkles (T-120) through the whole card pipeline with real Skia (CanvasKit WASM).
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import { AlphaType, ColorType, Skia } from '@shopify/react-native-skia';

import { createMask, maskToContours } from '@card-sharing/canvas-engine';
import { registry, type ParamValue } from '@card-sharing/effects-registry';

import { SCHEMES } from '../../../test/render';
import { buildTheme } from '../../../ui';
import { subjectPath, type CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { hasEffectRenderer } from '../compose';
import { MAX_SPARKLES, sparkles } from '../renderers/sparkles';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 128;

/** A disc subject of radius 32 in the middle of the card. */
function discScene(): CutScene {
  const data = new Uint8Array(SIZE * SIZE);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++)
      if ((x - 64) ** 2 + (y - 64) ** 2 <= 32 ** 2) data[y * SIZE + x] = 255;
  }
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  photo.getCanvas().clear(Skia.Color('#3366CC'));
  photo.flush();
  return {
    width: SIZE,
    height: SIZE,
    photo: photo.makeImageSnapshot(),
    subject: subjectPath(maskToContours(createMask(SIZE, SIZE, data)), 1),
  };
}

const scene = discScene();

const card = (
  params: Record<string, ParamValue>,
  cut = 'cut.raw',
  wholePhoto = false,
): CardDraft => ({
  cut: styleChoice(registry.get(cut)!),
  finish: null,
  effects: [styleChoice(registry.get('fx.sparkles')!, params)],
  wholePhoto,
  caption: '',
});

function render(
  draft: CardDraft,
  options: { background?: string; time?: number } = {},
): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  if (options.background) surface.getCanvas().clear(Skia.Color(options.background));
  drawCard(surface.getCanvas(), registry, draft, scene, {
    time: options.time,
    killed: new Set(['fx.paper-lift']),
  });
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

/** Opaque-ish pixels outside the subject disc (where only sparkles can draw). */
function outsideInk(pixels: Uint8Array): number {
  let n = 0;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      if ((x - 64) ** 2 + (y - 64) ** 2 > 34 ** 2 && pixels[(y * SIZE + x) * 4 + 3]! > 40) n++;
    }
  }
  return n;
}

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

describe('sparkles', () => {
  it('scatters particles around the subject outline, outside it', () => {
    expect(hasEffectRenderer(registry.get('fx.sparkles')!)).toBe(true);
    const items = sparkles(scene.subject!.path, 80, 3, 8, 5, 0, 'stars', scene.subject!.path);
    expect(items).toHaveLength(80);
    for (const item of items) {
      const r = Math.hypot(item.x - 64, item.y - 64);
      expect(r).toBeGreaterThan(29);
      expect(r).toBeLessThan(42);
    }
    expect(outsideInk(render(card({ amount: 120, seed: 4 })))).toBeGreaterThan(80);
  });

  it(`never draws more than ${MAX_SPARKLES} particles`, () => {
    expect(sparkles(scene.subject!.path, 5000, 2, 4, 1, 0, 'stars', null)).toHaveLength(
      MAX_SPARKLES,
    );
  });

  it('is deterministic per seed and grows with the amount', () => {
    const first = render(card({ seed: 9 }));
    expect(render(card({ seed: 9 }))).toEqual(first);
    expect(render(card({ seed: 10 }))).not.toEqual(first);
    expect(outsideInk(render(card({ seed: 9, amount: 300 })))).toBeGreaterThan(
      outsideInk(render(card({ seed: 9, amount: 20 }))),
    );
  });

  it('frames a whole-photo card from just inside its edge', () => {
    const pixels = render(card({ seed: 2, amount: 200, shape: 'confetti' }, 'cut.none', true));
    let tinted = 0;
    for (let y = 0; y < SIZE; y++) {
      for (let x = 0; x < SIZE; x++) {
        const [r, g, b] = at(pixels, x, y);
        if (r !== 51 || g !== 102 || b !== 204) tinted++;
      }
    }
    expect(tinted).toBeGreaterThan(100);
    expect(at(pixels, 64, 64)).toEqual([51, 102, 204, 255]);
  });

  it('twinkles with the effect clock but is a still at time 0 (reduced motion, bakes)', () => {
    const entry = registry.get('fx.sparkles')!;
    expect([entry.motion.reduced, entry.cost.liveAllowed]).toEqual(['static', false]);
    expect(render(card({ seed: 3 }), { time: 0 })).toEqual(render(card({ seed: 3 })));
    expect(render(card({ seed: 3 }), { time: 0.5 })).not.toEqual(render(card({ seed: 3 })));
    // The loop is seamless: a full period later the frame repeats.
    expect(render(card({ seed: 3 }), { time: 2 })).toEqual(render(card({ seed: 3 })));
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches for every preset', () => {
    const goldens = Object.fromEntries(
      ['stars', 'hearts', 'confetti'].map((shape) => [
        shape,
        signature(render(card({ shape, amount: 200, seed: 11 }), { background })),
      ]),
    );
    expect(goldens).toMatchSnapshot();
  });
});
