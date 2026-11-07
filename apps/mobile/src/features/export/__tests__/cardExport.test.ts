/**
 * Card export (T-127) with real Skia (CanvasKit WASM).
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import { AlphaType, ColorType, Skia, type SkImage } from '@shopify/react-native-skia';

import { registry } from '@card-sharing/effects-registry';

import { styleChoice } from '../../editor/editorState';
import {
  canShareImages,
  EXPORT_FILE_NAME,
  exportDraftPng,
  exportRenderPng,
  shareImage,
  shareImageOnWeb,
  toBase64,
  type BrowserExportDeps,
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

/** A browser with a document, object URLs and, optionally, the Web Share API. */
function fakeBrowser(share?: { canShare: boolean; result?: Error }) {
  const anchor = {
    href: '',
    download: '',
    rel: '',
    style: {} as Record<string, string>,
    click: jest.fn(),
    remove: jest.fn(),
  };
  const later: (() => void)[] = [];
  const blobs: Blob[] = [];
  const shared: File[][] = [];
  const deps: BrowserExportDeps = {
    Blob,
    File,
    document: {
      createElement: jest.fn(() => anchor),
      body: { appendChild: jest.fn() },
    } as unknown as BrowserExportDeps['document'],
    URL: {
      createObjectURL: jest.fn((blob: Blob) => {
        blobs.push(blob);
        return 'blob:card-1';
      }),
      revokeObjectURL: jest.fn(),
    },
    navigator: share
      ? {
          canShare: () => share.canShare,
          share: async ({ files }) => {
            shared.push(files);
            if (share.result) throw share.result;
          },
        }
      : {},
    later: (task) => void later.push(task),
  };
  return { deps, anchor, later, blobs, shared };
}

const named = (name: string) => Object.assign(new Error(name), { name });

describe('image export on the web (E1)', () => {
  const png = new Uint8Array([137, 80, 78]);

  it('is offered on the web and iOS, not yet on Android', () => {
    expect(canShareImages('web')).toBe(true);
    expect(canShareImages('ios')).toBe(true);
    expect(canShareImages('android')).toBe(false);
  });

  it('shares the PNG as a file when the browser can share files', async () => {
    const browser = fakeBrowser({ canShare: true });
    const rnShare = jest.fn();
    expect(await shareImage(png, rnShare as never, 'web', browser.deps)).toBe(true);
    expect(rnShare).not.toHaveBeenCalled();
    const [file] = browser.shared[0] ?? [];
    expect([file?.name, file?.type, file?.size]).toEqual([EXPORT_FILE_NAME, 'image/png', 3]);
    expect(browser.anchor.click).not.toHaveBeenCalled();
  });

  it('treats a cancelled share sheet as done, without downloading', async () => {
    const browser = fakeBrowser({ canShare: true, result: named('AbortError') });
    expect(await shareImageOnWeb(png, browser.deps)).toBe(true);
    expect(browser.anchor.click).not.toHaveBeenCalled();
  });

  it('downloads when the browser refuses the share (user activation expired)', async () => {
    const browser = fakeBrowser({ canShare: true, result: named('NotAllowedError') });
    expect(await shareImageOnWeb(png, browser.deps)).toBe(true);
    expect(browser.anchor.click).toHaveBeenCalledTimes(1);
  });

  it('downloads through an object URL and <a download> when files cannot be shared', async () => {
    const browser = fakeBrowser({ canShare: false });
    expect(await shareImageOnWeb(png, browser.deps)).toBe(true);
    expect(browser.shared).toHaveLength(0);
    expect(browser.blobs.map((blob) => [blob.type, blob.size])).toEqual([['image/png', 3]]);
    expect(browser.anchor).toMatchObject({ href: 'blob:card-1', download: EXPORT_FILE_NAME });
    expect(browser.anchor.click).toHaveBeenCalledTimes(1);
    expect(browser.anchor.remove).toHaveBeenCalledTimes(1);
    // The object URL is released later, not before the download has started.
    expect(browser.deps.URL?.revokeObjectURL).not.toHaveBeenCalled();
    browser.later.forEach((task) => task());
    expect(browser.deps.URL?.revokeObjectURL).toHaveBeenCalledWith('blob:card-1');
  });

  it('downloads in a browser without the Web Share API', async () => {
    const browser = fakeBrowser();
    expect(await shareImageOnWeb(png, browser.deps)).toBe(true);
    expect(browser.anchor.click).toHaveBeenCalledTimes(1);
  });

  it('reports failure when neither sharing nor downloading is possible', async () => {
    expect(await shareImageOnWeb(png, { Blob, File, navigator: {} })).toBe(false);
  });
});
