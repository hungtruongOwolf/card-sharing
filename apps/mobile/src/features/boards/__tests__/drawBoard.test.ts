/**
 * Draws board pins with real Skia (CanvasKit WASM) offscreen and probes pixels.
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { themes } from '@card-sharing/tokens';

import { pin } from '../../../test/boardFixtures';
import { PIN_FRAME } from '../boardModel';
import { boardPalette, drawPins, type DrawablePin } from '../drawBoard';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 400;
const palette = boardPalette(themes.light);

function solid(color: string, width: number, height: number): SkImage {
  const surface = Skia.Surface.Make(width, height)!;
  surface.getCanvas().clear(Skia.Color(color));
  surface.flush();
  return surface.makeImageSnapshot();
}

function render(items: DrawablePin[]): (x: number, y: number) => number[] {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  surface.getCanvas().clear(Skia.Color('#000000'));
  drawPins(surface.getCanvas(), items, palette);
  surface.flush();
  const pixels = surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
  return (x, y) => Array.from(pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 3));
}

const hex = (color: string) => {
  const c = Skia.Color(color);
  return [Math.round(c[0]! * 255), Math.round(c[1]! * 255), Math.round(c[2]! * 255)];
};

describe('drawPins', () => {
  it('draws a card texture "contain" inside the pin frame at the pin position', () => {
    // A square texture in a 3:4 frame: 240 x 240 centred on (200, 200).
    const probe = render([{ pin: pin({ x: 200, y: 200 }), image: solid('#3366CC', 64, 64) }]);
    expect(probe(200, 200)).toEqual([51, 102, 204]);
    expect(probe(200 - PIN_FRAME.width / 2 + 4, 200)).toEqual([51, 102, 204]);
    // Above the square, inside the frame: nothing drawn.
    expect(probe(200, 200 - PIN_FRAME.height / 2 + 4)).toEqual([0, 0, 0]);
  });

  it('applies scale and rotation around the pin centre', () => {
    const probe = render([
      { pin: pin({ x: 200, y: 200, scale: 0.5, rotation: 90 }), image: solid('#3366CC', 64, 64) },
    ]);
    expect(probe(200, 200)).toEqual([51, 102, 204]);
    expect(probe(200 + 70, 200)).toEqual([0, 0, 0]);
  });

  it('shows a ghost while the texture loads', () => {
    const probe = render([{ pin: pin({ x: 200, y: 200 }), image: null }]);
    expect(probe(200, 120)).toEqual(hex(palette.ghost));
  });

  it('draws a private tile as a neutral tile with a lock and nothing of the card', () => {
    const probe = render([{ pin: pin({ x: 200, y: 200 }, { kind: 'private_tile' }), image: null }]);
    expect(probe(200, 80)).toEqual(hex(palette.tile));
    expect(probe(200, 210)).toEqual(hex(palette.ink));
  });

  it('draws a removed card as an outline only', () => {
    const probe = render([{ pin: pin({ x: 200, y: 200 }, { kind: 'removed' }), image: null }]);
    expect(probe(200, 200)).toEqual([0, 0, 0]);
  });

  it('stacks pins in the order given', () => {
    const probe = render([
      { pin: pin({ x: 200, y: 200 }), image: solid('#3366CC', 64, 64) },
      { pin: pin({ x: 220, y: 200 }), image: solid('#CC3333', 64, 64) },
    ]);
    expect(probe(200, 200)).toEqual([204, 51, 51]);
  });
});
