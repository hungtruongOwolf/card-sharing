/**
 * Paper lift and the polaroid frame (T-118), drawn through the whole card pipeline with real Skia (CanvasKit WASM).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
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
import { polaroidWindow } from '../renderers/polaroid';
import { liftShadow } from '../renderers/paperLift';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 128;
const PHOTO = [51, 102, 204, 255];
const PAPER = [0xfb, 0xfa, 0xf5, 255];

/** A tall subject: a block from the top edge down to y = 100, x 40..88. */
function tallScene(): CutScene {
  const data = new Uint8Array(SIZE * SIZE);
  for (let y = 0; y < 100; y++) for (let x = 40; x < 88; x++) data[y * SIZE + x] = 255;
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

const scene = tallScene();

function draft(
  cut: string,
  effects: [string, Record<string, ParamValue>?][] = [],
  wholePhoto = false,
): CardDraft {
  return {
    cut: styleChoice(registry.get(cut)!),
    finish: null,
    effects: effects.map(([id, params]) => styleChoice(registry.get(id)!, params)),
    wholePhoto,
    caption: '',
  };
}

function render(
  card: CardDraft,
  options: { background?: string; killed?: ReadonlySet<string>; time?: number } = {},
): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  const canvas = surface.getCanvas();
  if (options.background) canvas.clear(Skia.Color(options.background));
  drawCard(canvas, registry, card, scene, options);
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

/** Coarse 8x8 signature for light/dark goldens (same shape as the cut style goldens). */
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

describe('paper lift (built into every sticker)', () => {
  const below: [number, number] = [64, 101];

  it('casts a soft shadow under a sticker card, never a picker choice', () => {
    expect(hasEffectRenderer(registry.get('fx.paper-lift')!)).toBe(true);
    const pixels = render(draft('cut.raw'));
    expect(at(pixels, 64, 50)).toEqual(PHOTO);
    const [r, g, b, a] = at(pixels, ...below);
    expect([r, g, b]).toEqual([0, 0, 0]);
    expect(a).toBeGreaterThan(10);
    expect(a).toBeLessThan(90);
    expect(at(pixels, 4, 120)[3]).toBe(0);
  });

  it('skips whole-photo cards and a remote kill switch', () => {
    expect(at(render(draft('cut.none', [], true)), 64, 64)).toEqual(PHOTO);
    expect(at(render(draft('cut.raw'), { killed: new Set(['fx.paper-lift']) }), ...below)[3]).toBe(
      0,
    );
  });

  it('is static: the effect clock does not change it (reduced motion needs no variant)', () => {
    expect(registry.get('fx.paper-lift')?.motion.reduced).toBe('static');
    expect(render(draft('cut.raw'), { time: 1.3 })).toEqual(render(draft('cut.raw')));
  });

  it('grows offset and blur with the lift height', () => {
    expect(liftShadow(0, 100)).toEqual({ dy: 0, sigma: 0 });
    const low = liftShadow(1, 100);
    const high = liftShadow(3, 100);
    expect(high.dy).toBeGreaterThan(low.dy);
    expect(high.sigma).toBeGreaterThan(low.sigma);
  });
});

describe('polaroid frame', () => {
  it('puts the card in a paper window with a thick caption border', () => {
    const window = polaroidWindow(SIZE, SIZE, true);
    expect(SIZE - window.y - window.height).toBeGreaterThan(window.y * 3);
    const pixels = render(draft('cut.none', [['frame.polaroid']], true));
    expect(at(pixels, 2, 2)).toEqual(PAPER);
    expect(at(pixels, 64, 120)).toEqual(PAPER);
    expect(at(pixels, 64, 64)).toEqual(PHOTO);
    const even = polaroidWindow(SIZE, SIZE, false);
    expect(SIZE - even.y - even.height).toBeCloseTo(even.y);
  });

  it('lets the subject pop out over the top border, only with a subject', () => {
    const top: [number, number] = [64, 3];
    const framed = render(draft('cut.raw', [['frame.polaroid']]));
    expect(at(framed, ...top)).toEqual(PAPER);
    const popped = render(draft('cut.raw', [['frame.polaroid', { popOut: true }]]));
    expect(at(popped, ...top)).toEqual(PHOTO);
    // Beside the subject the border stays paper.
    expect(at(popped, 10, 3)).toEqual(PAPER);
    const whole = render(draft('cut.none', [['frame.polaroid', { popOut: true }]], true));
    expect(at(whole, ...top)).toEqual(PAPER);
  });

  it('is a static frame with no live cost', () => {
    const entry = registry.get('frame.polaroid')!;
    expect([entry.cost.liveAllowed, entry.motion.reduced, entry.stackable]).toEqual([
      false,
      'static',
      false,
    ]);
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches for the sticker shadow and the polaroid variants', () => {
    const goldens = {
      paperLift: signature(render(draft('cut.diecut'), { background })),
      polaroid: signature(render(draft('cut.none', [['frame.polaroid']], true), { background })),
      polaroidPopOut: signature(
        render(draft('cut.diecut', [['frame.polaroid', { popOut: true, captionArea: false }]]), {
          background,
        }),
      ),
    };
    expect(goldens).toMatchSnapshot();
  });
});
