/**
 * Card export (T-127) with real Skia (CanvasKit WASM).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { registry } from '@card-sharing/effects-registry';

import { styleChoice } from '../../editor/editorState';
import {
  canShareImages,
  exportDraftPng,
  exportRenderPng,
  shareImage,
  toBase64,
} from '../cardExport';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

function solid(width: number, height: number, color: string): SkImage {
  const surface = Skia.Surface.Make(width, height)!;
  surface.getCanvas().clear(Skia.Color(color));
  surface.flush();
  return surface.makeImageSnapshot();
}

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

describe('card export', () => {
  it('keeps the render size and always adds the brand mark in the bottom-right corner', () => {
    const png = exportRenderPng(solid(200, 300, 'transparent'))!;
    const out = decode(png);
    expect([out.width, out.height]).toEqual([200, 300]);
    // Transparent everywhere except the mark (PNG with alpha, AC-23.1).
    expect(out.at(10, 10)[3]).toBe(0);
    let marked = 0;
    for (let y = 270; y < 300; y++)
      for (let x = 150; x < 200; x++) if (out.at(x, y)[3]! > 0) marked++;
    expect(marked).toBeGreaterThan(50);
  });

  it("bakes the user's own draft from the full photo at the export size", () => {
    const draft = {
      cut: styleChoice(registry.get('cut.none')!),
      finish: null,
      effects: [],
      wholePhoto: true,
      caption: '',
    };
    const png = exportDraftPng(
      { registry, draft, photo: solid(300, 400, '#3366CC'), mask: null },
      undefined,
      256,
    )!;
    const out = decode(png);
    expect([out.width, out.height]).toEqual([192, 256]);
    expect(out.at(96, 100)).toEqual([51, 102, 204, 255]);
  });

  it('shares a PNG data URI on iOS and reports Android as unsupported for now', async () => {
    const share = jest.fn(async () => ({ action: 'sharedAction' }));
    const png = new Uint8Array([1, 2, 3]);
    expect(await shareImage(png, share as never, 'ios')).toBe(true);
    expect(share).toHaveBeenCalledWith({ url: 'data:image/png;base64,AQID' });
    expect(await shareImage(png, share as never, 'android')).toBe(false);
    expect(canShareImages('android')).toBe(false);
    expect(toBase64(new Uint8Array([104, 105]))).toBe('aGk=');
  });
});
