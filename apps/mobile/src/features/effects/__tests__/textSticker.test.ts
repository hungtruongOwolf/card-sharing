/**
 * Text sticker (`overlay.text`, sprint 9) through the card pipeline with real Skia (CanvasKit WASM).
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import { AlphaType, ColorType, Skia } from '@shopify/react-native-skia';

import {
  registry,
  TEXT_FONTS,
  TEXT_PHRASES,
  TEXT_STYLES,
  type ParamValue,
} from '@card-sharing/effects-registry';
import type { FontRole } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../../i18n';
import type { CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { hasEffectRenderer } from '../compose';
import { setEffectTypeface } from '../effectFonts';
import { placementTargets } from '../placement';
import { inkOn, stickerText } from '../renderers/textSticker';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

declare const __dirname: string;
const { readFileSync } = jest.requireActual('fs') as { readFileSync(file: string): Uint8Array };
const { join } = jest.requireActual('path') as { join(...parts: string[]): string };

const FILES: Record<FontRole, string> = {
  display: 'bricolage-grotesque/BricolageGrotesque-Bold.ttf',
  body: 'dm-sans/DMSans-SemiBold.ttf',
  hand: 'caveat/Caveat-Medium.ttf',
  meme: 'anton/Anton-Regular.ttf',
  mono: 'dm-mono/DMMono-Medium.ttf',
};

beforeAll(() => {
  for (const [role, file] of Object.entries(FILES) as [FontRole, string][]) {
    const bytes = new Uint8Array(readFileSync(join(__dirname, '../../../../assets/fonts', file)));
    setEffectTypeface(role, Skia.Typeface.MakeFreeTypeFaceFromData(Skia.Data.fromBytes(bytes)));
  }
});
afterAll(() => {
  for (const role of Object.keys(FILES) as FontRole[]) setEffectTypeface(role, null);
});

const SIZE = 160;
const PHOTO = [51, 102, 204, 255];

function scene(): CutScene {
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  photo.getCanvas().clear(Skia.Color('#3366CC'));
  photo.flush();
  return { width: SIZE, height: SIZE, photo: photo.makeImageSnapshot(), subject: null };
}

const card = (params: Record<string, ParamValue> = {}): CardDraft => ({
  cut: styleChoice(registry.get('cut.none')!),
  finish: null,
  effects: [styleChoice(registry.get('overlay.text')!, params)],
  wholePhoto: true,
  caption: '',
});

function pixels(draft: CardDraft): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  drawCard(surface.getCanvas(), registry, draft, scene());
  surface.flush();
  return surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
}

/** Pixels that are no longer the photo, and their centre. */
function changed(data: Uint8Array) {
  let n = 0;
  let sx = 0;
  let sy = 0;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const i = (y * SIZE + x) * 4;
      const same = PHOTO.every((v, k) => Math.abs((data[i + k] ?? 0) - v) < 8);
      if (!same) {
        n++;
        sx += x;
        sy += y;
      }
    }
  }
  return { n, x: n ? sx / n : 0, y: n ? sy / n : 0 };
}

describe('text sticker', () => {
  it('has a renderer, a drag handle and copy for every phrase, font and style', () => {
    const entry = registry.get('overlay.text')!;
    expect(hasEffectRenderer(entry)).toBe(true);
    expect(placementTargets(entry)).toEqual(['body']);
    for (const phrase of TEXT_PHRASES) expect(stickerText(phrase)).not.toBe('');
    for (const font of TEXT_FONTS)
      expect(t(`effects.param.font.${font}` as MessageKey)).toBeTruthy();
    for (const style of TEXT_STYLES) {
      expect(t(`effects.param.textStyle.${style}` as MessageKey)).toBeTruthy();
    }
    expect(entry.tags).toContain('text');
  });

  it('draws only curated phrases', () => {
    expect(stickerText('not-a-phrase')).toBe('');
    expect(changed(pixels(card({ phrase: 'omg' }))).n).toBeGreaterThan(100);
  });

  it.each(TEXT_STYLES)('draws the %s style in every font where it is placed', (textStyle) => {
    for (const font of TEXT_FONTS) {
      const left = changed(pixels(card({ textStyle, font, x: 0.3, y: 0.3, angle: 0 })));
      const right = changed(pixels(card({ textStyle, font, x: 0.7, y: 0.7, angle: 0 })));
      expect([font, left.n > 50]).toEqual([font, true]);
      // The card is drawn slightly inset, so a 0.4 move is a bit under 0.4 of the surface.
      expect(right.x - left.x).toBeGreaterThan(0.33 * SIZE);
      expect(right.y - left.y).toBeGreaterThan(0.33 * SIZE);
    }
  });

  it('keeps a long phrase on the card at the biggest size', () => {
    const data = pixels(card({ phrase: 'mainCharacter', size: 2.5, angle: 0, textStyle: 'plain' }));
    // Nothing is drawn in the outer columns of the card.
    const photoAt = (x: number, y: number) => {
      const i = (y * SIZE + x) * 4;
      return Array.from(data.slice(i, i + 4));
    };
    const reference = photoAt(SIZE / 2, SIZE - 6);
    const near = (a: number[], b: number[]) => a.every((v, k) => Math.abs(v - (b[k] ?? 0)) <= 6);
    for (let y = 8; y < SIZE - 8; y += 4) {
      expect([y, near(photoAt(8, y), reference)]).toEqual([y, true]);
      expect([y, near(photoAt(SIZE - 9, y), reference)]).toEqual([y, true]);
    }
  });

  it('puts black ink on light pills and white ink on dark ones', () => {
    expect(inkOn('#FFFFFF')).toBe('#1A1A1A');
    expect(inkOn('#F2B705')).toBe('#1A1A1A');
    expect(inkOn('#1A1A1A')).toBe('#FFFFFF');
  });
});
