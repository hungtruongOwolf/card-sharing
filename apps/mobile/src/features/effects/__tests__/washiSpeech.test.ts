/**
 * Washi tape and the speech bubble (T-119) through the whole card pipeline with real Skia (CanvasKit WASM).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia } from '@shopify/react-native-skia';

import { registry, SPEECH_CAPTIONS, type ParamValue } from '@card-sharing/effects-registry';

import { SCHEMES } from '../../../test/render';
import { checkText } from '../../../text';
import { buildTheme } from '../../../ui';
import { CAPTION_MAX_GRAPHEMES } from '../../captions';
import type { CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { hasEffectRenderer } from '../compose';
import { setMemeTypeface } from '../effectFonts';
import { speechText } from '../renderers/speech';
import { TAPE_PALETTE } from '../tape';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

declare const __dirname: string;
const { readFileSync } = jest.requireActual('fs') as { readFileSync(file: string): Uint8Array };
const { join } = jest.requireActual('path') as { join(...parts: string[]): string };

const SIZE = 128;
const PHOTO = [51, 102, 204, 255];

const loadAnton = () =>
  Skia.Typeface.MakeFreeTypeFaceFromData(
    Skia.Data.fromBytes(
      new Uint8Array(
        readFileSync(join(__dirname, '../../../../assets/fonts/anton/Anton-Regular.ttf')),
      ),
    ),
  );

beforeAll(() => setMemeTypeface(loadAnton()));
afterAll(() => setMemeTypeface(null));

function scene(): CutScene {
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  photo.getCanvas().clear(Skia.Color('#3366CC'));
  photo.flush();
  return { width: SIZE, height: SIZE, photo: photo.makeImageSnapshot(), subject: null };
}

const card = (effects: [string, Record<string, ParamValue>?][]): CardDraft => ({
  cut: styleChoice(registry.get('cut.none')!),
  finish: null,
  effects: effects.map(([id, params]) => styleChoice(registry.get(id)!, params)),
  wholePhoto: true,
  caption: '',
});

function render(draft: CardDraft, background?: string): Uint8Array {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  if (background) surface.getCanvas().clear(Skia.Color(background));
  drawCard(surface.getCanvas(), registry, draft, scene());
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

const count = (pixels: Uint8Array, match: (rgba: number[]) => boolean) => {
  let n = 0;
  for (let i = 0; i < pixels.length; i += 4) if (match(Array.from(pixels.slice(i, i + 4)))) n++;
  return n;
};

describe('washi tape', () => {
  const plain = { pattern: 'plain', angle: 0, x: 0.5, y: 0.5, length: 0.6, seed: 7 };

  it('multiplies the tape colour over the photo where the strip lies', () => {
    expect(hasEffectRenderer(registry.get('overlay.washi')!)).toBe(true);
    const pixels = render(card([['overlay.washi', { ...plain, tapeColor: 'mint' }]]));
    const [, g, b] = at(pixels, 64, 64);
    // Multiply darkens: never brighter than the photo, and tinted by mint (more green than the photo).
    expect(b).toBeLessThanOrEqual(PHOTO[2]!);
    expect(g! / b!).toBeGreaterThan(PHOTO[1]! / PHOTO[2]!);
    expect(at(pixels, 64, 20)).toEqual(PHOTO);
    expect(TAPE_PALETTE.mint).toBe('#9ED9B8');
  });

  it('tears the same ends for the same seed and different ones for another', () => {
    const first = render(card([['overlay.washi', plain]]));
    expect(render(card([['overlay.washi', plain]]))).toEqual(first);
    expect(render(card([['overlay.washi', { ...plain, seed: 8 }]]))).not.toEqual(first);
  });

  it('stacks several strips', () => {
    const one = render(card([['overlay.washi', plain]]));
    const two = render(
      card([
        ['overlay.washi', plain],
        ['overlay.washi', { ...plain, y: 0.15 }],
      ]),
    );
    expect(at(two, 64, 19)).not.toEqual(at(one, 64, 19));
  });
});

describe('speech bubble', () => {
  const bubble = { caption: 'lol', x: 0.5, y: 0.25, tailX: 0.5, tailY: 0.8 };
  const isWhite = ([r, g, b]: number[]) => r! > 240 && g! > 240 && b! > 240;
  const isInk = ([r, g, b]: number[]) => r! < 60 && g! < 60 && b! < 60;

  it('draws a white bubble with dark text, its tail aiming at the anchor', () => {
    const pixels = render(card([['overlay.speech', bubble]]));
    expect(count(pixels, isWhite)).toBeGreaterThan(200);
    expect(count(pixels, isInk)).toBeGreaterThan(40);
    // The tail runs from the bubble down toward (64, 102) but stops short of it.
    expect(isWhite(at(pixels, 64, 60))).toBe(true);
    expect(at(pixels, 64, 110)).toEqual(PHOTO);
  });

  it('draws a meme caption without a bubble', () => {
    const pixels = render(card([['overlay.speech', { ...bubble, shape: 'meme', y: 0.85 }]]));
    expect(count(pixels, isWhite)).toBeGreaterThan(100);
    expect(at(pixels, 64, 20)).toEqual(PHOTO);
  });

  it('draws nothing while the effect font is not loaded', () => {
    setMemeTypeface(null);
    try {
      expect(at(render(card([['overlay.speech', bubble]])), 64, 32)).toEqual(PHOTO);
    } finally {
      setMemeTypeface(loadAnton());
    }
  });

  it('only draws curated captions that pass the text rules (SR-28)', () => {
    const spec = registry.get('overlay.speech')!.params.caption;
    expect(spec?.type === 'enum' && spec.values).toEqual(SPEECH_CAPTIONS);
    for (const caption of SPEECH_CAPTIONS) {
      const text = speechText(caption);
      expect([caption, checkText(text, { min: 1, max: CAPTION_MAX_GRAPHEMES }).issue]).toEqual([
        caption,
        null,
      ]);
    }
    expect(speechText('anything typed')).toBe('');
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches for tape, bubble and meme caption', () => {
    const goldens = {
      washi: signature(render(card([['overlay.washi', { seed: 3 }]]), background)),
      speech: signature(
        render(card([['overlay.speech', { x: 0.5, y: 0.3, tailX: 0.4, tailY: 0.7 }]]), background),
      ),
      meme: signature(render(card([['overlay.speech', { shape: 'meme', y: 0.85 }]]), background)),
    };
    expect(goldens).toMatchSnapshot();
  });
});
