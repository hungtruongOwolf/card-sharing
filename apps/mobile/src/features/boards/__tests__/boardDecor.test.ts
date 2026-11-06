/**
 * Draws board backdrops and decorations with real Skia (CanvasKit WASM) offscreen and probes pixels.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkCanvas } from '@shopify/react-native-skia';

import { registry } from '@card-sharing/effects-registry';
import { themes } from '@card-sharing/tokens';

import { en, type MessageKey } from '../../../i18n/messages/en';
import { SCHEMES } from '../../../test/render';
import { drawBackdrop, drawDecoration, hasBoardRenderer, TAPE_PALETTE } from '../boardDecor';
import { pinGeometry } from '../boardModel';
import { decorationSize } from '../decorationFrames';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 320;

function render(draw: (canvas: SkCanvas) => void): (x: number, y: number) => number[] {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  draw(surface.getCanvas());
  surface.flush();
  const pixels = surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
  return (x, y) => Array.from(pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4));
}

const backdrops = registry.byKind('backdrop');
const decorations = registry.byKind('decoration');
const area = { x: 0, y: 0, width: SIZE, height: SIZE };

describe('board registry entries', () => {
  it('have a renderer and catalog copy for every backdrop and decoration', () => {
    for (const entry of [...backdrops, ...decorations]) {
      expect(hasBoardRenderer(entry)).toBe(true);
      for (const key of [entry.name, entry.description, entry.a11yLabel]) {
        expect(en[key as MessageKey]).toEqual(expect.any(String));
      }
    }
  });
});

describe('drawBackdrop', () => {
  it.each(SCHEMES)('fills the whole area with every backdrop in %s mode', (scheme) => {
    for (const entry of backdrops) {
      const probe = render((c) =>
        drawBackdrop(c, entry.id, area, { scheme, color: themes[scheme] }),
      );
      expect(probe(10, 10)[3]).toBe(255);
      expect(probe(SIZE - 5, SIZE - 5)[3]).toBe(255);
    }
  });

  it('draws grid lines aligned to the board origin', () => {
    const ctx = { scheme: 'light' as const, color: themes.light };
    const grid = render((c) => drawBackdrop(c, 'bg.grid', area, ctx));
    // 48 board units: a vertical line at x = 48 and none half way between lines.
    expect(grid(48, 20)).not.toEqual(grid(24, 20));
  });

  it('falls back to the default paper for an unknown or missing backdrop', () => {
    const ctx = { scheme: 'light' as const, color: themes.light };
    const paper = render((c) => drawBackdrop(c, 'bg.paper', area, ctx));
    const unknown = render((c) => drawBackdrop(c, 'bg.from-the-future', area, ctx));
    const none = render((c) => drawBackdrop(c, null, area, ctx));
    expect(unknown(100, 100)).toEqual(paper(100, 100));
    expect(none(100, 100)).toEqual(paper(100, 100));
  });
});

describe('drawDecoration', () => {
  const ctx = { scheme: 'light' as const, color: themes.light };
  const centred = (draw: (c: SkCanvas) => void) =>
    render((c) => {
      c.translate(SIZE / 2, SIZE / 2);
      draw(c);
    });

  it('draws every decoration inside its frame and nothing outside it', () => {
    for (const entry of decorations) {
      const frame = decorationSize(entry.id);
      const probe = centred((c) => drawDecoration(c, { id: entry.id, params: {} }, ctx));
      let inked = 0;
      for (let y = 0; y < SIZE; y += 2) {
        for (let x = 0; x < SIZE; x += 2) {
          if ((probe(x, y)[3] ?? 0) === 0) continue;
          // Strokes and the push pin's shadow may spill a few px past the frame, never more.
          expect(Math.abs(x - SIZE / 2)).toBeLessThanOrEqual(frame.width / 2 + 8);
          expect(Math.abs(y - SIZE / 2)).toBeLessThanOrEqual(frame.height / 2 + 8);
          inked += 1;
        }
      }
      expect(inked).toBeGreaterThan(10);
    }
  });

  it('uses the tape colour param and clamps unknown values to the default', () => {
    const at = (color: unknown) =>
      centred((c) => drawDecoration(c, { id: 'deco.ribbon', params: { color } }, ctx))(
        SIZE / 2,
        SIZE / 2 + 20,
      );
    const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
    expect(at('sky').slice(0, 3)).toEqual(hex(TAPE_PALETTE.sky!));
    expect(at('neon').slice(0, 3)).toEqual(hex(TAPE_PALETTE.coral!));
  });

  it('draws nothing for an unknown decoration and gives it a placeholder frame', () => {
    const probe = centred((c) => drawDecoration(c, { id: 'deco.gone', params: {} }, ctx));
    expect(probe(SIZE / 2, SIZE / 2)[3]).toBe(0);
    expect(decorationSize('deco.gone')).toEqual({ width: 120, height: 120 });
  });

  it('gives decoration pins their own frame for hit tests and culling', () => {
    const geometry = pinGeometry({
      id: 'p',
      x: 10,
      y: 20,
      scale: 1,
      rotation: 0,
      zKey: 'a0',
      version: 1,
      content: { kind: 'decoration', decoration: { id: 'deco.ticket', version: 1, params: {} } },
    });
    expect(geometry).toMatchObject({ width: 260, height: 120 });
  });
});
