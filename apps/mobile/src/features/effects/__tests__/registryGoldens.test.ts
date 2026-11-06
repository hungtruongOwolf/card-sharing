/**
 * One golden per registry entry (T-124): every entry renders offscreen through the same path the app uses (card
 * entries through `drawCard`, board entries through the board decor), and its coarse signature is compared with the
 * committed snapshot. Adding an entry without a golden, or changing how one looks, fails CI (`jest --ci` never
 * writes snapshots); removing one also needs a ledger migration (packages/effects-registry/src/ledger.test.ts).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia, type SkCanvas } from '@shopify/react-native-skia';

import { createMask, maskToContours } from '@card-sharing/canvas-engine';
import {
  BOARD_KINDS,
  EFFECT_SLOT_KINDS,
  ENTRIES,
  registry,
  type EffectEntry,
} from '@card-sharing/effects-registry';

import { buildTheme } from '../../../ui';
import { drawBackdrop, drawDecoration, hasBoardRenderer } from '../../boards/boardDecor';
import { hasCutRenderer, subjectPath, type CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { hasFinishRenderer } from '../../editor/drawFinish';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { hasEffectRenderer } from '../compose';
import { setMemeTypeface } from '../effectFonts';

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
const theme = buildTheme('light', false);

beforeAll(() =>
  setMemeTypeface(
    Skia.Typeface.MakeFreeTypeFaceFromData(
      Skia.Data.fromBytes(
        new Uint8Array(
          readFileSync(join(__dirname, '../../../../assets/fonts/anton/Anton-Regular.ttf')),
        ),
      ),
    ),
  ),
);
afterAll(() => setMemeTypeface(null));

/** A photo with a disc subject (radius 36): every card entry draws on it. */
function buildScene(): CutScene {
  const data = new Uint8Array(SIZE * SIZE);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      if ((x - 64) ** 2 + (y - 70) ** 2 <= 36 ** 2) data[y * SIZE + x] = 255;
    }
  }
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  const canvas = photo.getCanvas();
  canvas.clear(Skia.Color('#3366CC'));
  const warm = Skia.Paint();
  warm.setColor(Skia.Color('#E8B53A'));
  canvas.drawRect(Skia.XYWHRect(0, SIZE / 2, SIZE, SIZE / 2), warm);
  photo.flush();
  return {
    width: SIZE,
    height: SIZE,
    photo: photo.makeImageSnapshot(),
    subject: subjectPath(maskToContours(createMask(SIZE, SIZE, data)), 1),
  };
}

const scene = buildScene();
const needsMask = (entry: EffectEntry) => entry.platforms.ios.requires.includes('subjectMask');
const isBuiltIn = (entry: EffectEntry) => entry.tags.includes('builtIn');
const isFinish = (entry: EffectEntry) => entry.tags.includes('finish') && !isBuiltIn(entry);

/** The draft that shows an entry best: as the cut, the finish, a built-in under a sticker, or the only effect. */
function draftFor(entry: EffectEntry): CardDraft {
  const base: CardDraft = {
    cut: styleChoice(registry.get('cut.none')!),
    finish: null,
    effects: [],
    wholePhoto: true,
    caption: '',
  };
  if (entry.kind === 'cutStyle') {
    return { ...base, cut: styleChoice(entry), wholePhoto: !needsMask(entry) };
  }
  if (isBuiltIn(entry))
    return { ...base, cut: styleChoice(registry.get('cut.raw')!), wholePhoto: false };
  if (isFinish(entry)) return { ...base, finish: styleChoice(entry) };
  return { ...base, effects: [styleChoice(entry, { x: 0.5, y: 0.5 })] };
}

function drawEntry(canvas: SkCanvas, entry: EffectEntry): void {
  const ctx = { scheme: 'light' as const, color: theme.color };
  if (entry.kind === 'backdrop') {
    drawBackdrop(canvas, entry.id, { x: 0, y: 0, width: SIZE, height: SIZE }, ctx);
  } else if (entry.kind === 'decoration') {
    canvas.save();
    canvas.translate(SIZE / 2, SIZE / 2);
    canvas.scale(0.5, 0.5);
    drawDecoration(canvas, { id: entry.id, params: {} }, ctx);
    canvas.restore();
  } else {
    // Built-ins are skipped elsewhere so each golden isolates its own entry.
    const killed = new Set(
      ENTRIES.filter((e) => isBuiltIn(e) && e.id !== entry.id).map((e) => e.id),
    );
    drawCard(canvas, registry, draftFor(entry), scene, { killed });
  }
}

function signature(entry: EffectEntry): string[] {
  const surface = Skia.Surface.Make(SIZE, SIZE)!;
  const canvas = surface.getCanvas();
  canvas.clear(Skia.Color(theme.color.bg.canvas));
  drawEntry(canvas, entry);
  surface.flush();
  const pixels = surface.makeImageSnapshot().readPixels(0, 0, {
    width: SIZE,
    height: SIZE,
    colorType: ColorType.RGBA_8888,
    alphaType: AlphaType.Unpremul,
  }) as Uint8Array;
  const rows: string[] = [];
  for (let y = SIZE / 16; y < SIZE; y += SIZE / 8) {
    const row: string[] = [];
    for (let x = SIZE / 16; x < SIZE; x += SIZE / 8) {
      const i = (y * SIZE + x) * 4;
      row.push(
        Array.from(pixels.slice(i, i + 4))
          .map((v) => v.toString(16).padStart(2, '0'))
          .join(''),
      );
    }
    rows.push(row.join(' '));
  }
  return rows;
}

function hasRenderer(entry: EffectEntry): boolean {
  if (BOARD_KINDS.includes(entry.kind)) return hasBoardRenderer(entry);
  if (entry.kind === 'cutStyle') return hasCutRenderer(entry);
  if (isFinish(entry)) return hasFinishRenderer(entry);
  if (entry.kind === 'filter' && !isBuiltIn(entry)) return hasFinishRenderer(entry);
  return EFFECT_SLOT_KINDS.includes(entry.kind) && hasEffectRenderer(entry);
}

describe('every registry entry (T-124)', () => {
  it('has a renderer in this build', () => {
    expect(ENTRIES.filter((entry) => !hasRenderer(entry)).map((entry) => entry.id)).toEqual([]);
  });

  it('matches its golden signature', () => {
    const goldens = Object.fromEntries(ENTRIES.map((entry) => [entry.id, signature(entry)]));
    // Every entry draws something on the canvas (a golden of an empty canvas would prove nothing).
    const empty = signature({ ...ENTRIES[0]!, id: 'deco.none', kind: 'decoration' });
    expect(
      ENTRIES.filter((entry) => goldens[entry.id]!.join() === empty.join()).map((e) => e.id),
    ).toEqual([]);
    expect(goldens).toMatchSnapshot();
  });
});
