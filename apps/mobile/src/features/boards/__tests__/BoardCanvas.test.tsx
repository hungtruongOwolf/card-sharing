/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Skia, type SkImage } from '@shopify/react-native-skia';
import { act, screen } from '@testing-library/react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';
import { makeMutable } from 'react-native-reanimated';

import type { Camera } from '@card-sharing/canvas-engine';
import { registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { cardItem, holo, pin } from '../../../test/boardFixtures';
import { renderThemed, SCHEMES } from '../../../test/render';
import { BoardCanvas } from '../BoardCanvas';
import { createTextureCache } from '../textureCache';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const RENDER_TIMEOUT_MS = 30_000;

function texture(): SkImage {
  const surface = Skia.Surface.Make(32, 32)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  return surface.makeImageSnapshot();
}

const shown = pin({ x: 150, y: 200 }, { kind: 'card', card: cardItem() });
const tile = pin({ x: 300, y: 500, zKey: 'a1' }, { kind: 'private_tile' });
const gone = pin({ x: 120, y: 600, zKey: 'a2' }, { kind: 'removed' });
const offscreen = pin({ x: 9000, y: 9000, zKey: 'a3' });
const shiny = pin(
  { x: 300, y: 250, zKey: 'a4' },
  { kind: 'card', card: cardItem({ finish: holo(), sender: { id: 'u2', displayName: null } }) },
);

async function renderBoard(
  options: { reducedMotion?: boolean; scheme?: 'light' | 'dark'; camera?: Camera } = {},
) {
  const load = jest.fn(async () => texture());
  const textures = createTextureCache({ load });
  const camera = makeMutable<Camera>(options.camera ?? { x: 0, y: 0, zoom: 1 });
  await renderThemed(
    <BoardCanvas
      pins={[shown, tile, gone, offscreen, shiny]}
      width={400}
      height={800}
      textures={textures}
      initialCamera={camera.get()}
      camera={camera}
      registry={registry}
    />,
    options,
  );
  await act(async () => {});
  return { load, camera, textures };
}

describe('BoardCanvas', () => {
  it.each(SCHEMES)(
    'labels every pin on screen for screen readers in %s mode, never off-screen ones',
    async (scheme) => {
      await renderBoard({ scheme });
      expect(screen.getByLabelText(t('boards.pin.card', { name: 'Mai' }))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.card.someone'))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.private'))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.removed'))).toBeTruthy();
      expect(screen.queryByTestId(`board-pin-${offscreen.id}`)).toBeNull();
      expect(screen.getByTestId('board-canvas').props.accessibilityLabel).toBe(
        t('boards.canvas.label', { count: 4 }),
      );
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'loads textures only for visible card pins, keyed by card id',
    async () => {
      const { load, textures } = await renderBoard();
      expect(load).toHaveBeenCalledTimes(2);
      const urls = load.mock.calls.map((call) => (call as unknown[])[0]);
      expect(urls).not.toContain(
        offscreen.content.kind === 'card' ? offscreen.content.card.media!.renderUrl : '',
      );
      expect(textures.get(shown.content.kind === 'card' ? shown.content.card.id : '')).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'pans and zooms the camera with gestures and re-labels pins where they now are',
    async () => {
      const { camera } = await renderBoard();
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pan'), [
          { state: State.BEGAN, translationX: 0, translationY: 0 },
          { state: State.ACTIVE, translationX: 0, translationY: 0 },
          { translationX: -100, translationY: 0 },
          { state: State.END, translationX: -100, translationY: 0 },
        ]);
      });
      expect(camera.get().x).toBeCloseTo(100);
      const label = screen.getByTestId(`board-pin-${shown.id}`);
      expect(label.props.style.start).toBeLessThan(100);

      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pinch'), [
          { state: State.BEGAN, scale: 1, focalX: 0, focalY: 0 },
          { state: State.ACTIVE, scale: 1, focalX: 0, focalY: 0 },
          { scale: 2, focalX: 0, focalY: 0 },
          { state: State.END, scale: 2, focalX: 0, focalY: 0 },
        ]);
      });
      expect(camera.get().zoom).toBeCloseTo(2);
    },
    RENDER_TIMEOUT_MS,
  );
});
