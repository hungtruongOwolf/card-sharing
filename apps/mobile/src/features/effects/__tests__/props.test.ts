/**
 * The crown and props family (T-123) through the whole card pipeline with real Skia (CanvasKit WASM).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { AlphaType, ColorType, Skia } from '@shopify/react-native-skia';

import { registry, stackingErrors, type ParamValue } from '@card-sharing/effects-registry';

import { SCHEMES } from '../../../test/render';
import { buildTheme } from '../../../ui';
import type { CutScene } from '../../cut-styles';
import { drawCard } from '../../editor/cardScene';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { DEFAULT_ANCHORS, placeEffect, type Anchors } from '../anchors';
import { hasEffectRenderer } from '../compose';
import { placementTargets } from '../placement';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const SIZE = 128;
const PHOTO = [51, 102, 204, 255];
const PROPS = ['prop.crown', 'prop.party-hat', 'prop.sunglasses', 'prop.mustache'];

function scene(): CutScene {
  const photo = Skia.Surface.Make(SIZE, SIZE)!;
  photo.getCanvas().clear(Skia.Color('#3366CC'));
  photo.flush();
  return { width: SIZE, height: SIZE, photo: photo.makeImageSnapshot(), subject: null };
}

const card = (id: string, params: Record<string, ParamValue> = {}): CardDraft => ({
  cut: styleChoice(registry.get('cut.none')!),
  finish: null,
  effects: [styleChoice(registry.get(id)!, params)],
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

/** Centre of the pixels that differ from the photo. */
function inkCentre(pixels: Uint8Array): { x: number; y: number; n: number } {
  let sx = 0;
  let sy = 0;
  let n = 0;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const [r, g, b] = at(pixels, x, y);
      if (r !== PHOTO[0] || g !== PHOTO[1] || b !== PHOTO[2]) {
        sx += x;
        sy += y;
        n++;
      }
    }
  }
  return { x: sx / n, y: sy / n, n };
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

describe('props family', () => {
  it('has a renderer and a drag handle for every prop', () => {
    for (const id of PROPS) {
      const entry = registry.get(id)!;
      expect([id, hasEffectRenderer(entry), placementTargets(entry)]).toEqual([id, true, ['body']]);
    }
  });

  it('auto-places each prop on its anchor and scales it to the head', () => {
    const anchors: Anchors = { ...DEFAULT_ANCHORS, headWidth: 0.2, source: 'mask' };
    const crown = placeEffect(registry.get('prop.crown')!, anchors);
    expect([crown.x, crown.y]).toEqual([anchors.top.x, anchors.top.y]);
    expect(crown.size).toBeCloseTo(0.34 / 2);
    const glasses = placeEffect(registry.get('prop.sunglasses')!, anchors);
    expect(glasses.y).toBeCloseTo(anchors.eyes[0].y);
    const mustache = placeEffect(registry.get('prop.mustache')!, anchors);
    expect(mustache.y).toBe(anchors.mouth.y);
  });

  it('draws a prop where it is placed, and moves with it', () => {
    const left = inkCentre(render(card('prop.sunglasses', { x: 0.3, y: 0.4 })));
    const right = inkCentre(render(card('prop.sunglasses', { x: 0.7, y: 0.4 })));
    expect(left.n).toBeGreaterThan(200);
    expect(left.x).toBeCloseTo(0.3 * SIZE, -1);
    expect(right.x - left.x).toBeCloseTo(0.4 * SIZE, -1);
  });

  it('stands hats on the anchor (drawn above it)', () => {
    const hat = inkCentre(render(card('prop.crown', { x: 0.5, y: 0.6 })));
    expect(hat.y).toBeLessThan(0.6 * SIZE);
    expect(at(render(card('prop.crown', { x: 0.5, y: 0.6 })), 64, 90)).toEqual(PHOTO);
  });

  it('keeps one hat on the head: crown and party hat replace each other', () => {
    expect(stackingErrors([registry.get('prop.crown')!, registry.get('prop.party-hat')!])).toEqual([
      'prop.crown conflicts with prop.party-hat',
    ]);
    expect(stackingErrors([registry.get('prop.crown')!, registry.get('prop.sunglasses')!])).toEqual(
      [],
    );
  });

  it('ships the ten MVP effects of effects-catalog.md §3.3 except the two with live beams/flames', () => {
    const mvp = [
      'cut.diecut',
      'fx.paper-lift',
      'fx.holo',
      'frame.polaroid',
      'overlay.washi',
      'prop.crown',
      'fx.sparkles',
      'overlay.speech',
    ];
    expect(mvp.filter((id) => !registry.get(id))).toEqual([]);
  });
});

describe.each(SCHEMES)('golden signatures on the %s canvas', (scheme) => {
  const background = buildTheme(scheme, false).color.bg.canvas;
  it('matches for every prop', () => {
    const goldens = Object.fromEntries(
      PROPS.map((id) => [
        id,
        signature(render(card(id, { x: 0.5, y: 0.6, size: 0.6 }), background)),
      ]),
    );
    expect(goldens).toMatchSnapshot();
  });
});
